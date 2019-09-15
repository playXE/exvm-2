#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Register(u8);

impl Register {
    pub const fn high(self) -> u8 {
        (self.0 >> 3) & 1
    }
    pub const fn low(self) -> u8 {
        self.0 & 7
    }
    pub const fn is(self, r: Register) -> bool {
        self.0 == r.0
    }

    pub const fn code(self) -> u8 {
        self.0
    }
}
