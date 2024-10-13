use rtrb::{Consumer, Producer, RingBuffer};
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
        loop {
            match self.sender.push(frame.clone()) {
                Ok(_) => break,
                Err(e) => {
                    // Handle the error, e.g., buffer is full
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
                Err(_) => {
                    // Buffer is empty or disconnected; wait and retry
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }
}

impl<E> IntoIterator for DecoderInputQueue<E> {
    type Item = Result<XcoderDecoderInputFrame, E>;
    type IntoIter = Self;

    fn into_iter(self) -> Self::IntoIter {
        self
    }
}
