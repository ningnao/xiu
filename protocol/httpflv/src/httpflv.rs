use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use chrono::Local;
use hyper::{Body, Request};
use tokio::sync::{Mutex, oneshot};
use {
    super::{
        define::{tag_type, HttpResponseDataProducer},
        errors::{HttpFLvError, HttpFLvErrorValue},
    },
    crate::rtmp::{
        cache::metadata::MetaData,
        session::errors::{SessionError, SessionErrorValue},
    },
    bytes::BytesMut,
    std::net::SocketAddr,
    streamhub::define::{
        FrameData, FrameDataReceiver, NotifyInfo, StreamHubEvent, StreamHubEventSender,
        SubDataType, SubscribeType, SubscriberInfo,
    },
    streamhub::{
        stream::StreamIdentifier,
        utils::{RandomDigitCount, Uuid},
    },
    tokio::sync::mpsc,
    xflv::muxer::{FlvMuxer, HEADER_LENGTH},
};

pub struct HttpFlv {
    app_name: String,
    stream_name: String,

    muxer: FlvMuxer,

    event_producer: StreamHubEventSender,
    data_consumer: FrameDataReceiver,
    http_response_data_producer: HttpResponseDataProducer,
    subscriber_id: Uuid,
    request_url: String,
    remote_addr: SocketAddr,

    req: Request<Body>,
    enabled_nonce: bool,
    need_record: bool,
    file_handler: Option<File>,
    subscribe_token: Option<String>,
    first_video_metadata: bool,
    nonce_map: Arc<Mutex<HashMap<String, i64>>>,
}

impl HttpFlv {
    pub fn new(
        app_name: String,
        stream_name: String,
        event_producer: StreamHubEventSender,
        http_response_data_producer: HttpResponseDataProducer,
        req: Request<Body>,
        remote_addr: SocketAddr,
        enabled_nonce: bool,
        need_record: bool,
        subscribe_token: Option<String>,
        nonce_map: Arc<Mutex<HashMap<String, i64>>>,
    ) -> Self {
        let (_, data_consumer) = mpsc::unbounded_channel();
        let subscriber_id = Uuid::new(RandomDigitCount::Four);

        Self {
            app_name,
            stream_name,
            muxer: FlvMuxer::new(),
            data_consumer,
            event_producer,
            http_response_data_producer,
            subscriber_id,
            request_url: req.uri().to_string(),
            remote_addr,
            req,
            enabled_nonce,
            need_record,
            file_handler: None,
            subscribe_token,
            first_video_metadata: true,
            nonce_map,
        }
    }

    pub async fn run(&mut self) -> Result<(), HttpFLvError> {
        self.subscribe_from_rtmp_channels().await?;

        let mut token = None;
        let mut nonce = None;
        if self.need_record {
            let flv_folder = format!("./{}/{}/flv", &self.app_name, &self.stream_name);
            fs::create_dir_all(&flv_folder).unwrap();

            //default value
            let mut flv_name = format!(
                "{}-{}-{}.flv",
                self.stream_name,
                Local::now().format("%Y-%m-%d-%H-%M-%S").to_string(),
                &uuid::Uuid::new_v4().to_string()[..6],
            );

            //set flv name specified by user
            if let Some(params) = self.req.uri().query() {
                for param in params.split("&") {
                    let entry: Vec<_> = param.split("=").collect();
                    if entry.len() == 2 {
                        if entry[0].to_string() == "file_name".to_string() {
                            flv_name = format!("{}.flv", entry[1].to_string());
                        }
                        if entry[0].to_string() == "token".to_string() {
                            token = Some(entry[1].to_string());
                        }
                        if entry[0].to_string() == "nonce".to_string() {
                            nonce = Some(entry[1].to_string());
                        }
                    }
                }
            }
            // validate token
            validate_token(&self.subscribe_token, &token)?;
            if self.enabled_nonce {
                validate_nonce(&self.nonce_map, &nonce).await?;
            }

            let file_path = format!("{}/{}", flv_folder, flv_name);
            if Path::new(&file_path).exists() {
                return Err(HttpFLvError {
                    value: HttpFLvErrorValue::FileExist
                })
            }

            self.file_handler = Some(File::create(&file_path).unwrap());
        }

        self.send_media_stream().await?;

        Ok(())
    }

    pub async fn send_media_stream(&mut self) -> Result<(), HttpFLvError> {
        self.muxer.write_flv_header()?;
        self.muxer.write_previous_tag_size(0)?;

        self.flush_response_data()?;

        let mut retry_count = 0;
        //write flv body
        loop {
            if let Some(data) = self.data_consumer.recv().await {
                if let Err(err) = self.write_flv_tag(data) {
                    if let HttpFLvErrorValue::MpscSendError(err_in) = &err.value {
                        if err_in.is_disconnected() {
                            log::info!("write_flv_tag: {}", err_in);
                            break;
                        }
                    }
                    log::error!("write_flv_tag err: {}", err);
                    retry_count += 1;
                } else {
                    retry_count = 0;
                }
            } else {
                retry_count += 1;
            }
            if retry_count > 10 {
                break;
            }
        }

        if let Some(file_handler) = &mut self.file_handler {
            let mut timestamp_bytes = [0u8;3];
            let timestamp = self.muxer.timestamp_delta;
            timestamp_bytes[0] = (timestamp >> 16) as u8;
            timestamp_bytes[1] = (timestamp >> 8) as u8;
            timestamp_bytes[2] = timestamp as u8;

            let mut end : Vec<u8> = Vec::new();
            end.extend_from_slice(&[0x09, 0x00, 0x00, 0x05]);
            end.extend_from_slice(&timestamp_bytes);
            end.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x17, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10]);

            file_handler.write_all(&end)?;
        }
        self.unsubscribe_from_rtmp_channels().await
    }

    pub fn write_flv_tag(&mut self, channel_data: FrameData) -> Result<(), HttpFLvError> {
        let (common_data, common_timestamp, tag_type) = match channel_data {
            FrameData::Audio { timestamp, data } => (data, timestamp, tag_type::AUDIO),
            FrameData::Video { timestamp, data } => (data, timestamp, tag_type::VIDEO),
            FrameData::MetaData { timestamp, data } => {
                let mut metadata = MetaData::new();
                metadata.save(&data);
                let data = metadata.remove_set_data_frame()?;

                (data, timestamp, tag_type::SCRIPT_DATA_AMF)
            }
            _ => {
                log::error!("should not be here!!!");
                (BytesMut::new(), 0, 0)
            }
        };

        // only record first video metadata
        if common_timestamp == 0 && tag_type == tag_type::VIDEO {
            if self.first_video_metadata {
                self.first_video_metadata = false;
            } else {
                return Ok(());
            }
        }

        let common_data_len = common_data.len() as u32;

        self.muxer
            .write_flv_tag_header(tag_type, common_data_len, common_timestamp)?;
        self.muxer.write_flv_tag_body(common_data)?;
        self.muxer
            .write_previous_tag_size(common_data_len + HEADER_LENGTH)?;

        self.flush_response_data()?;

        Ok(())
    }

    pub fn flush_response_data(&mut self) -> Result<(), HttpFLvError> {
        let data = self.muxer.writer.extract_current_bytes();

        if let Some(file_handler) = &mut self.file_handler {
            file_handler.write_all(data.as_ref())?;
        }

        self.http_response_data_producer.start_send(Ok(data))?;

        Ok(())
    }

    pub async fn unsubscribe_from_rtmp_channels(&mut self) -> Result<(), HttpFLvError> {
        let sub_info = SubscriberInfo {
            id: self.subscriber_id,
            sub_type: SubscribeType::PlayerHttpFlv,
            sub_data_type: SubDataType::Frame,
            notify_info: NotifyInfo {
                request_url: self.request_url.clone(),
                remote_addr: self.remote_addr.to_string(),
            },
        };

        let identifier = StreamIdentifier::Rtmp {
            app_name: self.app_name.clone(),
            stream_name: self.stream_name.clone(),
        };

        let subscribe_event = StreamHubEvent::UnSubscribe {
            identifier,
            info: sub_info,
        };
        if let Err(err) = self.event_producer.send(subscribe_event) {
            log::error!("unsubscribe_from_channels err {}", err);
        }

        Ok(())
    }

    pub async fn subscribe_from_rtmp_channels(&mut self) -> Result<(), HttpFLvError> {
        let sub_info = SubscriberInfo {
            id: self.subscriber_id,
            sub_type: SubscribeType::PlayerHttpFlv,
            sub_data_type: SubDataType::Frame,
            notify_info: NotifyInfo {
                request_url: self.request_url.clone(),
                remote_addr: self.remote_addr.to_string(),
            },
        };

        let identifier = StreamIdentifier::Rtmp {
            app_name: self.app_name.clone(),
            stream_name: self.stream_name.clone(),
        };

        let (event_result_sender, event_result_receiver) = oneshot::channel();

        let subscribe_event = StreamHubEvent::Subscribe {
            identifier,
            info: sub_info,
            result_sender: event_result_sender,
        };

        let rv = self.event_producer.send(subscribe_event);
        if rv.is_err() {
            let session_error = SessionError {
                value: SessionErrorValue::SendFrameDataErr,
            };
            return Err(HttpFLvError {
                value: HttpFLvErrorValue::SessionError(session_error),
            });
        }

        let receiver = event_result_receiver.await??.frame_receiver.unwrap();
        self.data_consumer = receiver;

        Ok(())
    }
}

fn validate_token(server_token: &Option<String>, token: &Option<String>) -> Result<(), HttpFLvError>{
    if let Some(server_token) = server_token {
        match token {
            Some(token) => {
                if token.as_str() != server_token {
                    return Err(HttpFLvError {
                        value: HttpFLvErrorValue::Forbidden,
                    });
                }
            }
            None => {
                return Err(HttpFLvError {
                    value: HttpFLvErrorValue::NoToken,
                });
            }
        }
    }

    Ok(())
}

async fn validate_nonce(nonce_map: &Arc<Mutex<HashMap<String, i64>>>, nonce: &Option<String>) -> Result<(), HttpFLvError> {
    match nonce {
        Some(nonce) => {
            if let Some(timestamp) = nonce_map.lock().await.remove(nonce) {
                if timestamp < Local::now().timestamp_millis() {
                    return Err(HttpFLvError {
                        value: HttpFLvErrorValue::InvalidNonce,
                    });
                }
            } else {
                return Err(HttpFLvError {
                    value: HttpFLvErrorValue::Forbidden,
                });
            }
        }
        None => {
            return Err(HttpFLvError {
                value: HttpFLvErrorValue::Forbidden,
            });
        }
    }

    Ok(())
}