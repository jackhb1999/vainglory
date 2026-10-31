//! 确定性数学工具：定点数、固定时间步工具

use fixed::types::I32F32;
use bevy::prelude::*;


/// 定点数类型别名（32位整数部分 + 32位小数部分）
pub type Fixed = I32F32;

/// 确定性 Vec2
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FixedVec2 {
    pub x: Fixed,
    pub y: Fixed,
}

impl FixedVec2 {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x: Fixed::from_num(x),
            y: Fixed::from_num(y),
        }
    }
    
    pub fn to_f32(&self) -> Vec2 {
        Vec2::new(self.x.to_num::<f32>(), self.y.to_num::<f32>())
    }
    
    pub fn from_f32(v: Vec2) -> Self {
        Self::new(v.x, v.y)
    }
    
    pub fn length(&self) -> Fixed {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    
    pub fn normalize(&self) -> Self {
        let len = self.length();
        if len == Fixed::ZERO {
            Self::ZERO
        } else {
            Self {
                x: self.x / len,
                y: self.y / len,
            }
        }
    }
    
    pub const ZERO: Self = Self { x: Fixed::ZERO, y: Fixed::ZERO };
}

impl std::ops::Add for FixedVec2 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { x: self.x + other.x, y: self.y + other.y }
    }
}

impl std::ops::Sub for FixedVec2 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self { x: self.x - other.x, y: self.y - other.y }
    }
}

impl std::ops::Mul<Fixed> for FixedVec2 {
    type Output = Self;
    fn mul(self, scalar: Fixed) -> Self {
        Self { x: self.x * scalar, y: self.y * scalar }
    }
}

impl std::ops::Mul<f32> for FixedVec2 {
    type Output = Self;
    fn mul(self, scalar: f32) -> Self {
        Self { x: self.x * Fixed::from_num(scalar), y: self.y * Fixed::from_num(scalar) }
    }
}

/// 确定性 Vec3
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FixedVec3 {
    pub x: Fixed,
    pub y: Fixed,
    pub z: Fixed,
}

impl FixedVec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            x: Fixed::from_num(x),
            y: Fixed::from_num(y),
            z: Fixed::from_num(z),
        }
    }
    
    pub fn to_f32(&self) -> Vec3 {
        Vec3::new(self.x.to_num::<f32>(), self.y.to_num::<f32>(), self.z.to_num::<f32>())
    }
    
    pub fn from_f32(v: Vec3) -> Self {
        Self::new(v.x, v.y, v.z)
    }
    
    pub fn distance(&self, other: &Self) -> Fixed {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
    
    pub const ZERO: Self = Self { x: Fixed::ZERO, y: Fixed::ZERO, z: Fixed::ZERO };
}

/// 固定时间步工具
#[derive(Clone, Copy, Debug, PartialEq, Resource)]
pub struct TickClock {
    pub current_tick: u64,
    pub tick_rate: u64,
    pub accumulator: f32,
}

impl Default for TickClock {
    fn default() -> Self {
        Self::new(60)
    }
}

impl TickClock {
    pub fn new(tick_rate: u64) -> Self {
        Self {
            current_tick: 0,
            tick_rate,
            accumulator: 0.0,
        }
    }
    
    pub fn tick_duration(&self) -> f32 {
        1.0 / self.tick_rate as f32
    }
    
    pub fn advance(&mut self, delta_time: f32) -> u64 {
        self.accumulator += delta_time;
        let tick_duration = self.tick_duration();
        let mut ticks_advanced = 0;
        
        while self.accumulator >= tick_duration {
            self.accumulator -= tick_duration;
            self.current_tick += 1;
            ticks_advanced += 1;
        }
        
        ticks_advanced
    }
    
    pub fn current_time(&self) -> f64 {
        self.current_tick as f64 / self.tick_rate as f64
    }
    
    pub fn interpolation_alpha(&self) -> f32 {
        self.accumulator / self.tick_duration()
    }
}

/// 插值工具
pub fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub fn lerp_vec3(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a.lerp(b, t.clamp(0.0, 1.0))
}

pub fn lerp_quat(a: Quat, b: Quat, t: f32) -> Quat {
    a.slerp(b, t.clamp(0.0, 1.0))
}

/// 确定性随机数生成器（用于回放一致性）
pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }
    
    /// Xorshift64* 算法
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    
    pub fn next_range(&mut self, min: f32, max: f32) -> f32 {
        min + self.next_f32() * (max - min)
    }
    
    pub fn next_bool(&mut self, probability: f32) -> bool {
        self.next_f32() < probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_vec2() {
        let a = FixedVec2::new(3.0, 4.0);
        let b = FixedVec2::new(1.0, 2.0);
        
        let sum = a + b;
        assert_eq!(sum.to_f32(), Vec2::new(4.0, 6.0));
        
        let len = a.length();
        assert!((len.to_num::<f32>() - 5.0).abs() < 0.001);
    }
    
    #[test]
    fn test_tick_clock() {
        let mut clock = TickClock::new(60);
        assert_eq!(clock.tick_duration(), 1.0 / 60.0);
        
        let ticks = clock.advance(1.0 / 60.0);
        assert_eq!(ticks, 1);
        assert_eq!(clock.current_tick, 1);
        
        // 累积不足一帧
        let ticks = clock.advance(0.001);
        assert_eq!(ticks, 0);
        assert_eq!(clock.current_tick, 1);
    }
    
    #[test]
    fn test_deterministic_rng() {
        let mut rng1 = DeterministicRng::new(12345);
        let mut rng2 = DeterministicRng::new(12345);
        
        for _ in 0..100 {
            assert_eq!(rng1.next_u64(), rng2.next_u64());
        }
    }
}
