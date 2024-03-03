use streamhub::errors::StreamHubError;

use {
    failure::Fail, futures::channel::mpsc::SendError, std::fmt,
    tokio::sync::oneshot::error::RecvError, xflv::amf0::errors::Amf0WriteError,
    xflv::errors::FlvMuxerError,
};

#[derive(Debug)]
pub struct ServerError {
    pub value: ServerErrorValue,
}

#[derive(Debug, Fail)]
pub enum ServerErrorValue {
    #[fail(display = "server error")]
    Error,
}

pub struct HttpFLvError {
    pub value: HttpFLvErrorValue,
}

#[derive(Debug, Fail)]
pub enum HttpFLvErrorValue {
    #[fail(display = "server error")]
    Error,
    #[fail(display = "flv muxer error")]
    MuxerError(FlvMuxerError),
    #[fail(display = "amf write error")]
    Amf0WriteError(Amf0WriteError),
    #[fail(display = "metadata error")]
    MpscSendError(SendError),
    #[fail(display = "event execute error: {}", _0)]
    ChannelError(StreamHubError),
    #[fail(display = "tokio: oneshot receiver err: {}", _0)]
    RecvError(#[cause] RecvError),
    #[fail(display = "channel recv error")]
    ChannelRecvError,
    #[fail(display = "send frame data error")]
    SendFrameDataErr,

    #[fail(display = "write file error:{}", _0)]
    IOError(std::io::Error),
    #[fail(display = "no token")]
    NoToken,
    #[fail(display = "forbidden")]
    Forbidden,
    #[fail(display = "file exist")]
    FileExist,
    #[fail(display = "invalid nonce")]
    InvalidNonce,
}

impl From<FlvMuxerError> for HttpFLvError {
    fn from(error: FlvMuxerError) -> Self {
        HttpFLvError {
            value: HttpFLvErrorValue::MuxerError(error),
        }
    }
}

impl From<SendError> for HttpFLvError {
    fn from(error: SendError) -> Self {
        HttpFLvError {
            value: HttpFLvErrorValue::MpscSendError(error),
        }
    }
}

impl From<Amf0WriteError> for HttpFLvError {
    fn from(error: Amf0WriteError) -> Self {
        HttpFLvError {
            value: HttpFLvErrorValue::Amf0WriteError(error),
        }
    }
}

impl From<StreamHubError> for HttpFLvError {
    fn from(error: StreamHubError) -> Self {
        HttpFLvError {
            value: HttpFLvErrorValue::ChannelError(error),
        }
    }
}

impl From<RecvError> for HttpFLvError {
    fn from(error: RecvError) -> Self {
        HttpFLvError {
            value: HttpFLvErrorValue::RecvError(error),
        }
    }
}

impl fmt::Display for HttpFLvError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.value, f)
    }
}

impl From<std::io::Error> for HttpFLvError {
    fn from(error: std::io::Error) -> Self {
        HttpFLvError {
            value: HttpFLvErrorValue::IOError(error),
        }
    }
}
