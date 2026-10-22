use crossbeam_channel::{bounded, Receiver, Sender};
use std::sync::Arc;

pub const DEFAULT_CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4 MB per block
pub const DEFAULT_POOL_CAPACITY: usize = 8;             // 8 blocks = 32 MB working set

#[derive(Clone)]
pub struct CarveBuffer {
    pub id: usize,
    pub lba_start: u64,
    pub valid_bytes: usize,
    pub data: Vec<u8>,
}

impl CarveBuffer {
    pub fn new(id: usize, capacity: usize) -> Self {
        Self {
            id,
            lba_start: 0,
            valid_bytes: 0,
            data: vec![0u8; capacity],
        }
    }
}

pub struct RingBufferPool {
    empty_tx: Sender<CarveBuffer>,
    empty_rx: Receiver<CarveBuffer>,
    filled_tx: Sender<CarveBuffer>,
    filled_rx: Receiver<CarveBuffer>,
    buffer_size: usize,
}

impl RingBufferPool {
    pub fn new(capacity: usize, buffer_size: usize) -> Self {
        let (empty_tx, empty_rx) = bounded(capacity);
        let (filled_tx, filled_rx) = bounded(capacity);

        // Pre-allocate zeroed 4MB/8MB buffers
        for id in 0..capacity {
            empty_tx.send(CarveBuffer::new(id, buffer_size)).unwrap();
        }

        Self {
            empty_tx,
            empty_rx,
            filled_tx,
            filled_rx,
            buffer_size,
        }
    }

    pub fn buffer_size(&self) -> usize {
        self.buffer_size
    }

    /// Reader (Producer) acquires an empty buffer from the pool
    pub fn acquire_empty(&self) -> Result<CarveBuffer, crossbeam_channel::RecvError> {
        self.empty_rx.recv()
    }

    /// Reader (Producer) submits a filled buffer to the workers
    pub fn submit_filled(&self, buffer: CarveBuffer) -> Result<(), crossbeam_channel::SendError<CarveBuffer>> {
        self.filled_tx.send(buffer)
    }

    /// Workers (Consumers) receive filled buffers for carving
    pub fn receive_filled(&self) -> Result<CarveBuffer, crossbeam_channel::RecvError> {
        self.filled_rx.recv()
    }

    /// Workers (Consumers) return processed buffers back to the pool
    pub fn release_buffer(&self, mut buffer: CarveBuffer) -> Result<(), crossbeam_channel::SendError<CarveBuffer>> {
        buffer.valid_bytes = 0;
        self.empty_tx.send(buffer)
    }

    pub fn get_worker_receiver(&self) -> Receiver<CarveBuffer> {
        self.filled_rx.clone()
    }

    pub fn get_recycler_sender(&self) -> Sender<CarveBuffer> {
        self.empty_tx.clone()
    }
}
