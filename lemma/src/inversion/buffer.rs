// The buffer is used to store the values that are in the domain, and the values that are not in the domain.
// It is not a floating point value buffer, but a Haar wavelet buffer, describing the regions of the domain that are in the buffer.
// It provides multi resolution analysis of the domain.

#[cfg(target_arch = "x86_64")]
const MAX_PRECISION: u64 = u64::MAX;

#[cfg(target_arch = "wasm32")]
const MAX_PRECISION: u32 = u32::MAX;

const BUFFER_SIZE: usize = 256;

pub struct Buffer {
    values: [u8; BUFFER_SIZE],
    
    #[cfg(target_arch = "x86_64")]
    precision: u64,
    #[cfg(target_arch = "wasm32")]
    precision: u32,
}

impl Buffer {
    pub fn new() -> Self {
        Self { values: [0; BUFFER_SIZE], precision: MAX_PRECISION }
    }

    pub fn set_values(&mut self, values: [u8; BUFFER_SIZE]) {
        self.values = values;
    }

    pub fn get_values(&self) -> [u8; BUFFER_SIZE] {
        self.values
    }

    #[cfg(target_arch = "x86_64")]
    pub fn get_precision(&self) -> u64 {
        self.precision
    }

    #[cfg(target_arch = "wasm32")]
    pub fn get_precision(&self) -> u32 {
        self.precision
    }
    
    #[cfg(target_arch = "x86_64")]
    pub fn set_precision(&mut self, precision: u64) {
        self.precision = precision;
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_precision(&mut self, precision: u32) {
        self.precision = precision;
    }

    pub fn get_peak_values(&self) -> [u8; BUFFER_SIZE] {
        let mut peak_values = [0; BUFFER_SIZE];
        let mut current_value = 0;
        for i in 0..BUFFER_SIZE {
            current_value += self.values[i] as u64;
        }
        peak_values[i] = current_value as u8;
        peak_values
    }
}