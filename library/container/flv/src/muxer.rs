use {
    super::errors::FlvMuxerError, byteorder::BigEndian, bytes::BytesMut,
    bytesio::bytes_writer::BytesWriter,
};

const FLV_HEADER: [u8; 9] = [
    0x46, // 'F'
    0x4c, //'L'
    0x56, //'V'
    0x01, //version
    0x05, //00000101  audio tag  and video tag
    0x00, 0x00, 0x00, 0x09, //flv header size
]; // 9
pub const HEADER_LENGTH: u32 = 11;
pub struct FlvMuxer {
    pub writer: BytesWriter,
    first_flag: bool,
    timestamp_start: u32,
    pub timestamp_delta: u32,
}

impl Default for FlvMuxer {
    fn default() -> Self {
        Self::new()
    }
}

impl FlvMuxer {
    pub fn new() -> Self {
        Self {
            writer: BytesWriter::new(),
            first_flag: true,
            timestamp_start: 0,
            timestamp_delta: 0,
        }
    }

    pub fn write_flv_header(&mut self) -> Result<(), FlvMuxerError> {
        self.writer.write(&FLV_HEADER)?;
        Ok(())
    }

    pub fn write_flv_tag_header(
        &mut self,
        tag_type: u8,
        data_size: u32,
        timestamp: u32,
    ) -> Result<(), FlvMuxerError> {
        //save timestamp
        if timestamp > 0 {
            if self.first_flag {
                self.timestamp_start = timestamp;
                self.first_flag = false;
            } else if timestamp < self.timestamp_start {
                self.timestamp_start = timestamp;
                log::warn!("timestamp smaller than first timestamp");
            }
            self.timestamp_delta = timestamp - self.timestamp_start;
        }
        //tag type
        self.writer.write_u8(tag_type)?;
        //data size
        self.writer.write_u24::<BigEndian>(data_size)?;
        //timestamp
        self.writer.write_u24::<BigEndian>(timestamp & 0xffffff)?;
        //timestamp extended.
        let timestamp_ext = (timestamp >> 24 & 0xff) as u8;
        self.writer.write_u8(timestamp_ext)?;
        //stream id
        self.writer.write_u24::<BigEndian>(0)?;

        Ok(())
    }

    pub fn write_flv_tag_body(&mut self, body: BytesMut) -> Result<(), FlvMuxerError> {
        self.writer.write(&body[..])?;
        Ok(())
    }

    pub fn write_previous_tag_size(&mut self, size: u32) -> Result<(), FlvMuxerError> {
        self.writer.write_u32::<BigEndian>(size)?;
        Ok(())
    }
}
