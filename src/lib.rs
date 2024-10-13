use rtrb::{Consumer, Producer, RingBuffer};
use std::mem;
use std::thread;
use std::time::Duration;
use xcoder_quadra::decoder::XcoderDecoderInputFrame;

pub struct DecoderInputQueue<E> {
    receiver: Consumer<Result<XcoderDecoderInputFrame, E>>,
}

pub struct DecoderInputQueueProducer<E> {
    sender: Producer<Result<XcoderDecoderInputFrame, E>>,
}

impl<E> DecoderInputQueue<E> {
    pub fn new(capacity: usize) -> (Self, DecoderInputQueueProducer<E>) {
        let (producer, consumer) = RingBuffer::new(capacity);
        (
            Self { receiver: consumer },
            DecoderInputQueueProducer { sender: producer },
        )
    }
}

impl<E> DecoderInputQueueProducer<E> {
    pub fn push(&mut self, frame: Result<XcoderDecoderInputFrame, E>) {
        let mut frame = frame;
        loop {
            match self.sender.push(frame) {
                Ok(_) => break,
                Err(rtrb::PushError::Full(returned_frame)) => {
                    frame = returned_frame;
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }
}

impl<E> Iterator for DecoderInputQueue<E> {
    type Item = Result<XcoderDecoderInputFrame, E>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.receiver.pop() {
                Ok(frame) => return Some(frame),
                Err(rtrb::PopError::Empty) => {
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }
}

pub fn read_frames(buf: &[u8]) -> Vec<Result<XcoderDecoderInputFrame, std::io::Error>> {
    let nalus: Vec<_> = h264::iterate_annex_b(&buf).collect();
    let mut ret = vec![];
    let mut buffer = vec![];
    let mut h264_counter = h264::AccessUnitCounter::new();
    for nalu in nalus {
        let is_new_frame = {
            let before = h264_counter.count();
            h264_counter.count_nalu(nalu).unwrap();
            h264_counter.count() != before
        };
        if is_new_frame && !buffer.is_empty() {
            ret.push(Ok(XcoderDecoderInputFrame {
                data: mem::take(&mut buffer),
                pts: ret.len() as _,
                dts: ret.len() as _,
            }));
        }
        buffer.extend_from_slice(&[0, 0, 0, 1]);
        buffer.extend_from_slice(nalu);
    }
    if !buffer.is_empty() {
        ret.push(Ok(XcoderDecoderInputFrame {
            data: mem::take(&mut buffer),
            pts: ret.len() as _,
            dts: ret.len() as _,
        }));
    }
    ret
}

#[cfg(test)]
mod test {
    use super::*;
    use std::io::Read;
    use std::io::Write;
    use xcoder_quadra::decoder::{XcoderDecoder, XcoderDecoderCodec, XcoderDecoderConfig};
    use xcoder_quadra::encoder::{XcoderEncoder, XcoderEncoderCodec, XcoderEncoderConfig};
    use xcoder_quadra::linux_impl::XcoderPixelFormat;
    use xcoder_quadra::scaler::{XcoderScaler, XcoderScalerConfig};

    #[test]
    fn test_iterator() {
        let path = "testdata/output.h264";
        let mut f = std::fs::File::open(path).unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();

        let (mut decoder_input_queue, mut producer_queue) = DecoderInputQueue::new(1024);

        let frames = read_frames(&buf);

        // Initialize the decoder with the iterator
        let mut decoder = XcoderDecoder::new(
            XcoderDecoderConfig {
                width: 1920,
                height: 1080,
                codec: XcoderDecoderCodec::H264,
                bit_depth: 8,
                fps: 29.97,
                hardware_id: None,
                multicore_joint_mode: false,
            },
            decoder_input_queue, // Pass the queue as the input iterator
        )
        .expect("Failed to create decoder");

        let mut scaler = XcoderScaler::new(XcoderScalerConfig {
            hardware: decoder.hardware(),
            width: 640,
            height: 360,
            bit_depth: 8,
        })
        .unwrap();

        let mut encoder = XcoderEncoder::new(XcoderEncoderConfig {
            width: 640,
            height: 360,
            fps: 29.97,
            bitrate: None,
            codec: XcoderEncoderCodec::H264 {
                profile: None,
                level_idc: None,
            },
            bit_depth: 8,
            pixel_format: XcoderPixelFormat::Yuv420Planar,
            hardware: Some(decoder.hardware()),
            multicore_joint_mode: false,
        })
        .unwrap();

        let mut encoded_frames = 0;
        let mut encoded = vec![];

        for frame in frames {
            producer_queue.push(frame);
            dbg!("pushed frame");
            if let Some(decoded_frame) = decoder
                .try_read_decoded_frame()
                .expect("Failed to read frame")
            {
                dbg!("got frame");
                let frame = scaler.scale(&decoded_frame.into()).unwrap();
                if let Some(output) = encoder.encode_hardware_frame((), frame).unwrap() {
                    encoded.append(&mut output.encoded_frame.expect("frame was not dropped").data);
                    encoded_frames += 1;
                }
                dbg!("scaled frame");
            }
        }

        while let Some(output) = encoder.flush().unwrap() {
            encoded.append(&mut output.encoded_frame.expect("frame was not dropped").data);
            encoded_frames += 1;
        }

        std::fs::File::create("testdata/output.h264")
            .unwrap()
            .write_all(&encoded)
            .unwrap();
        drop(producer_queue);
        dbg!("dropped producer");
    }
}
