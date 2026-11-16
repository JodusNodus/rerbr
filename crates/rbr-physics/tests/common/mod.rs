//! Helpers the test files share: hex bytes, and a memory whose clones share one byte map.

#![allow(dead_code)]

use rbr_physics::mem::{Backend, Mem};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Two hex digits per byte.
pub fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

#[derive(Default)]
struct SharedBytes(Mutex<BTreeMap<u32, u8>>);

impl Backend for SharedBytes {
    fn read(&self, at: u32, out: &mut [u8]) -> bool {
        let bytes = self.0.lock().unwrap();
        (0..out.len() as u32).all(|i| match bytes.get(&(at + i)) {
            Some(&byte) => {
                out[i as usize] = byte;
                true
            }
            None => false,
        })
    }

    fn write(&self, at: u32, src: &[u8]) {
        let mut bytes = self.0.lock().unwrap();
        for (i, &byte) in src.iter().enumerate() {
            bytes.insert(at + i as u32, byte);
        }
    }
}

/// An empty memory on a shared byte map: its clones are the same memory, so a host can keep one.
pub fn shared_mem() -> Mem {
    Mem::with_backend(Arc::new(SharedBytes::default()))
}
