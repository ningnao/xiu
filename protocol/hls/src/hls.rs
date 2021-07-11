use core::time;

use byteorder::BigEndian;
use libflv::define::FlvDemuxerData;
use libflv::demuxer::FlvAudioDemuxer;
use libflv::demuxer::FlvVideoDemuxer;
use libflv::muxer::HEADER_LENGTH;
use networkio::bytes_writer::BytesWriter;
use rtmp::amf0::amf0_writer::Amf0Writer;
use rtmp::cache::metadata::MetaData;
use rtmp::session::common::SessionInfo;
use rtmp::session::define::SessionSubType;
use rtmp::session::errors::SessionError;
use rtmp::session::errors::SessionErrorValue;
use rtmp::utils::print;
use {
    bytes::BytesMut,
    rtmp::channels::define::{
        ChannelData, ChannelDataConsumer, ChannelDataProducer, ChannelEvent, ChannelEventProducer,
    },
    std::time::Duration,
    tokio::{
        sync::{mpsc, oneshot, Mutex},
        time::sleep,
    },
};

use super::errors::HlsError;
use super::errors::HlsErrorValue;

pub struct Hls {
    app_name: String,
    stream_name: String,
    video_demuxer: FlvVideoDemuxer,
    audio_demuxer: FlvAudioDemuxer,
    event_producer: ChannelEventProducer,
    data_consumer: ChannelDataConsumer,
    pts: u64,
    dts: u64,
}

impl Hls {
    pub fn new(
        app_name: String,
        stream_name: String,
        event_producer: ChannelEventProducer,
    ) -> Self {
        let (_, data_consumer) = mpsc::unbounded_channel();

        Self {
            app_name,
            stream_name,
            video_demuxer: FlvVideoDemuxer::new(),
            audio_demuxer: FlvAudioDemuxer::new(),
            data_consumer,
            event_producer,
            pts: 0,
            dts: 0,
        }
    }

    pub async fn run(&mut self) -> Result<(), HlsError> {
        self.subscribe_from_rtmp_channels(self.app_name.clone(), self.stream_name.clone(), 50)
            .await?;

        self.process_media_data().await?;

        Ok(())
    }

    pub async fn process_media_data(&mut self) -> Result<(), HlsError> {
        loop {
            if let Some(data) = self.data_consumer.recv().await {
                match data {
                    ChannelData::Audio { timestamp, data } => {
                        let audio_data = self.audio_demuxer.demuxer(timestamp, data)?;
                    }
                    ChannelData::Video { timestamp, data } => {
                        let video_data = self.video_demuxer.demuxer(timestamp, data)?;
                    }
                    ChannelData::MetaData { timestamp, data } => {}
                }
            }
        }
    }

    pub fn flush_response_data(&mut self) -> Result<(), HlsError> {
        Ok(())
    }

    pub async fn subscribe_from_rtmp_channels(
        &mut self,
        app_name: String,
        stream_name: String,
        session_id: u64,
    ) -> Result<(), HlsError> {
        let mut retry_count: u8 = 0;

        loop {
            let (sender, receiver) = oneshot::channel();

            let session_info = SessionInfo {
                session_id: session_id,
                session_sub_type: SessionSubType::Player,
            };

            let subscribe_event = ChannelEvent::Subscribe {
                app_name: app_name.clone(),
                stream_name: stream_name.clone(),
                session_info: session_info,
                responder: sender,
            };
            println!("httpflv begin send subscribe");
            let rv = self.event_producer.send(subscribe_event);
            match rv {
                Err(_) => {
                    let session_error = SessionError {
                        value: SessionErrorValue::SendChannelDataErr,
                    };
                    println!("httpflv send subscribe error");
                    return Err(HlsError {
                        value: HlsErrorValue::SessionError(session_error),
                    });
                }
                _ => {}
            }

            match receiver.await {
                Ok(consumer) => {
                    self.data_consumer = consumer;
                    break;
                }
                Err(_) => {
                    if retry_count > 10 {
                        let session_error = SessionError {
                            value: SessionErrorValue::SubscribeCountLimitReach,
                        };
                        return Err(HlsError {
                            value: HlsErrorValue::SessionError(session_error),
                        });
                    }
                }
            }

            sleep(Duration::from_millis(800)).await;
            retry_count = retry_count + 1;
        }

        Ok(())
    }
}
