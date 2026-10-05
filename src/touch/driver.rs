//! MPR121 register driver. All addresses are seven-bit I2C addresses.
use embedded_hal_async::{delay::DelayNs, i2c::I2c};

pub const ADDRESS: u8 = 0x5a;
pub const TOUCH_THRESHOLD: u8 = 15;
pub const RELEASE_THRESHOLD: u8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Bus,
    UnexpectedRegister,
    OverCurrent,
    OutOfRange,
    Timeout,
}

#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    pub touched: u16,
    pub filtered: [u16; 12],
    pub baseline: [u16; 12],
}

impl Snapshot {
    /// Signed signal change in ADC counts, not capacitance in pF.
    pub fn delta(&self) -> [i16; 12] {
        core::array::from_fn(|i| self.baseline[i] as i16 - self.filtered[i] as i16)
    }

    pub fn decode(bytes: &[u8; 42]) -> Result<Self, Fault> {
        let status = u16::from_le_bytes([bytes[0], bytes[1]]);
        if status & 0x8000 != 0 {
            return Err(Fault::OverCurrent);
        }
        if bytes[2] != 0 || bytes[3] != 0 {
            return Err(Fault::OutOfRange);
        }
        let mut filtered = [0; 12];
        let mut baseline = [0; 12];
        for i in 0..12 {
            filtered[i] = u16::from_le_bytes([bytes[4 + 2 * i], bytes[5 + 2 * i]]) & 0x3ff;
            baseline[i] = u16::from(bytes[0x1e + i]) << 2;
        }
        Ok(Self {
            touched: status & 0x0fff,
            filtered,
            baseline,
        })
    }
}

pub struct Mpr121<I> {
    bus: I,
    address: u8,
}

impl<I: I2c> Mpr121<I> {
    pub fn new(bus: I, address: u8) -> Self {
        assert!((0x5a..=0x5d).contains(&address));
        Self { bus, address }
    }

    async fn write(&mut self, register: u8, value: u8) -> Result<(), Fault> {
        self.bus
            .write(self.address, &[register, value])
            .await
            .map_err(|_| Fault::Bus)
    }

    async fn read(&mut self, register: u8, bytes: &mut [u8]) -> Result<(), Fault> {
        self.bus
            .write_read(self.address, &[register], bytes)
            .await
            .map_err(|_| Fault::Bus)
    }

    pub async fn init(&mut self, delay: &mut impl DelayNs) -> Result<(), Fault> {
        self.write(0x80, 0x63).await?;
        delay.delay_ms(2).await;
        let mut value = [0];
        self.read(0x5d, &mut value).await?;
        // No WHO_AM_I register: check the documented reset value, not a fabricated ID.
        if value[0] != 0x24 {
            return Err(Fault::UnexpectedRegister);
        }
        self.write(0x5e, 0).await?;
        for (register, value) in [
            (0x2b, 1),
            (0x2c, 1),
            (0x2d, 0),
            (0x2e, 0),
            (0x2f, 1),
            (0x30, 1),
            (0x31, 0xff),
            (0x32, 2),
            (0x33, 0),
            (0x34, 0),
            (0x35, 0),
        ] {
            self.write(register, value).await?;
        }
        for electrode in 0..12 {
            self.write(0x41 + electrode * 2, TOUCH_THRESHOLD).await?;
            self.write(0x42 + electrode * 2, RELEASE_THRESHOLD).await?;
        }
        self.write(0x5b, 0).await?;
        self.write(0x5c, 0x10).await?; // FFI=6, CDC=16 uA.
        self.write(0x5d, 0x22).await?; // CDT=0.5 us, SFI=4, ESI=4 ms (~16 ms response).
        self.write(0x5e, 0x8c).await?; // Track baseline, seed upper 5 bits, enable 12 electrodes.
        self.read(0x5e, &mut value).await?;
        if value[0] != 0x8c {
            return Err(Fault::UnexpectedRegister);
        }
        Ok(())
    }

    pub async fn sample(&mut self) -> Result<Snapshot, Fault> {
        // Reading 0x00/0x01 acknowledges IRQ. Read all 12 channels, including 8..11.
        let mut bytes = [0; 42];
        self.read(0, &mut bytes).await?;
        Snapshot::decode(&bytes)
    }
}
