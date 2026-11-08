//! P-V4 8-bit 程序化音频。
//!
//! 全部波形在**运行时由代码合成**，不读取任何音频文件、不内嵌字节常量，
//! 满足 PRES-1 / PRES-2 与 `TC-CBT-12`（发布树无音频二进制）。
//! 播放层通过自定义 `Decodable` 把内存里的 PCM 样本直接交给引擎音频后端，
//! 因此不需要任何解码 feature 或外部资源。
//!
//! headless / 无声环境：`Assets<ChipSource>` 未注册时 `play_sfx` 直接丢弃请求，
//! 不影响游戏逻辑（§8.6 静默降级口径）。

use bevy::asset::Asset;
use bevy::audio::{
    AudioPlayer, ChannelCount, Decodable, PlaybackSettings, Sample, SampleRate, Source,
};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use std::num::NonZero;
use std::sync::Arc;
use std::time::Duration;

/// 芯片音采样率：22.05 kHz，兼顾 8-bit 质感与内存占用。
pub const SAMPLE_RATE: u32 = 22_050;
/// 4-bit 幅度量化级数（16 阶），产生可辨认的阶梯状波形。
pub const QUANT_LEVELS: f32 = 16.0;
/// 单帧最多播放的音效数，避免团战时音频实体刷屏（§7 性能预算）。
pub const MAX_SFX_PER_FRAME: usize = 4;

/// 音效标识。新增音效 = 加一个枚举项 + 一行 `SFX_TABLE`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SfxId {
    Attack,
    Cast,
    Kill,
    TurretDown,
    Capture,
    Overtime,
    Victory,
}

/// 三种经典芯片波形。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChipWave {
    Square,
    Triangle,
    Noise,
}

/// 一个音效的完整合成参数（波形 + 频率滑移 + ADSR + 增益）。
#[derive(Clone, Copy, Debug)]
pub struct SfxDef {
    pub id: SfxId,
    pub wave: ChipWave,
    pub freq: f32,
    pub freq_end: f32,
    /// ADSR 各段占总时长的比例；四段之和必须为 1.0
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub duration: f32,
    pub gain: f32,
}

/// 音效参数表：**唯一数值来源**，与 `SfxId` 一一对应。
pub const SFX_TABLE: [SfxDef; 7] = [
    SfxDef {
        id: SfxId::Attack,
        wave: ChipWave::Square,
        freq: 320.0,
        freq_end: 180.0,
        attack: 0.05,
        decay: 0.25,
        sustain: 0.25,
        release: 0.45,
        duration: 0.11,
        gain: 0.55,
    },
    SfxDef {
        id: SfxId::Cast,
        wave: ChipWave::Triangle,
        freq: 220.0,
        freq_end: 660.0,
        attack: 0.10,
        decay: 0.20,
        sustain: 0.40,
        release: 0.30,
        duration: 0.20,
        gain: 0.60,
    },
    SfxDef {
        id: SfxId::Kill,
        wave: ChipWave::Noise,
        freq: 180.0,
        freq_end: 60.0,
        attack: 0.02,
        decay: 0.30,
        sustain: 0.20,
        release: 0.48,
        duration: 0.28,
        gain: 0.70,
    },
    SfxDef {
        id: SfxId::TurretDown,
        wave: ChipWave::Square,
        freq: 140.0,
        freq_end: 45.0,
        attack: 0.02,
        decay: 0.28,
        sustain: 0.25,
        release: 0.45,
        duration: 0.36,
        gain: 0.65,
    },
    SfxDef {
        id: SfxId::Capture,
        wave: ChipWave::Triangle,
        freq: 392.0,
        freq_end: 523.0,
        attack: 0.08,
        decay: 0.22,
        sustain: 0.40,
        release: 0.30,
        duration: 0.24,
        gain: 0.55,
    },
    SfxDef {
        id: SfxId::Overtime,
        wave: ChipWave::Square,
        freq: 440.0,
        freq_end: 220.0,
        attack: 0.05,
        decay: 0.20,
        sustain: 0.50,
        release: 0.25,
        duration: 0.34,
        gain: 0.60,
    },
    SfxDef {
        id: SfxId::Victory,
        wave: ChipWave::Triangle,
        freq: 392.0,
        freq_end: 784.0,
        attack: 0.10,
        decay: 0.20,
        sustain: 0.45,
        release: 0.25,
        duration: 0.42,
        gain: 0.70,
    },
];

pub fn sfx_def(id: SfxId) -> &'static SfxDef {
    SFX_TABLE
        .iter()
        .find(|d| d.id == id)
        .unwrap_or(&SFX_TABLE[0])
}

/// ADSR 包络，输入为归一化进度 `[0,1]`，输出 `[0,1]`。
pub fn envelope(p: f32, d: &SfxDef) -> f32 {
    let p = p.clamp(0.0, 1.0);
    let a_end = d.attack;
    let d_end = a_end + d.decay;
    let s_end = d_end + d.sustain;
    if p < a_end {
        if a_end <= 0.0 { 1.0 } else { p / a_end }
    } else if p < d_end {
        let k = (p - a_end) / d.decay.max(1e-4);
        1.0 - 0.35 * k
    } else if p < s_end {
        0.65
    } else {
        let k = (p - s_end) / d.release.max(1e-4);
        0.65 * (1.0 - k.clamp(0.0, 1.0))
    }
}

/// 确定性 LCG，供噪声波形使用；同一 `SfxDef` 每次合成结果完全一致。
fn noise_next(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    ((*state >> 16) & 0xFFFF) as f32 / 32_767.5 - 1.0
}

/// 核心合成器：逐样本生成波形 → 包络 → 增益 → 4-bit 量化。
/// 纯函数，不访问任何外部状态，可在单测中完整验证。
pub fn synthesize(d: &SfxDef) -> Vec<Sample> {
    let n = (SAMPLE_RATE as f32 * d.duration).max(1.0) as usize;
    let mut out = Vec::with_capacity(n);
    let mut rng = 0x9E37_79B9_u32 ^ (d.freq as u32).wrapping_mul(31);
    for i in 0..n {
        let p = i as f32 / n as f32;
        let t = i as f32 / SAMPLE_RATE as f32;
        let freq = d.freq + (d.freq_end - d.freq) * p;
        let phase = (t * freq).fract();
        let raw = match d.wave {
            ChipWave::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            ChipWave::Triangle => 4.0 * (phase - 0.5).abs() - 1.0,
            ChipWave::Noise => noise_next(&mut rng),
        };
        let v = (raw * envelope(p, d) * d.gain).clamp(-1.0, 1.0);
        out.push((v * QUANT_LEVELS).round() / QUANT_LEVELS);
    }
    out
}

/// 转为 16-bit PCM（用于测试与未来导出；不写任何文件）。
pub fn pcm_i16(samples: &[Sample]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32_767.0) as i16)
        .collect()
}

/// 事件特效种子 → 音效的**显式**映射表。种子段与
/// `docs/06_VISUAL_SPEC.md` §11.2.4 的事件覆盖表一致。
/// 未登记的种子（野怪死亡、波次、状态等高频事件）不发声，避免刷屏。
pub const SFX_EVENT_MAP: [(u32, SfxId); 11] = [
    (13, SfxId::Cast),
    (110, SfxId::Cast),
    (207, SfxId::Cast),
    (304, SfxId::Cast),
    (81, SfxId::Victory),
    (94, SfxId::Victory),
    (82, SfxId::Kill),
    (83, SfxId::TurretDown),
    (95, SfxId::TurretDown),
    (85, SfxId::Capture),
    (93, SfxId::Overtime),
];

pub fn sfx_for_event(seed: u32) -> Option<SfxId> {
    SFX_EVENT_MAP
        .iter()
        .find(|(s, _)| *s == seed)
        .map(|(_, id)| *id)
}

/// 自定义可解码音频资产：内存 PCM，无需文件格式与解码 feature。
#[derive(Asset, TypePath, Clone, Debug)]
pub struct ChipSource {
    pub samples: Arc<[Sample]>,
}

impl ChipSource {
    pub fn from_id(id: SfxId) -> Self {
        Self {
            samples: synthesize(sfx_def(id)).into(),
        }
    }
}

/// 单声道 PCM 迭代器，同时实现 `rodio::Source`。
pub struct ChipDecoder {
    samples: Arc<[Sample]>,
    index: usize,
}

impl Iterator for ChipDecoder {
    type Item = Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.samples.get(self.index).copied();
        if value.is_some() {
            self.index += 1;
        }
        value
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.samples.len().saturating_sub(self.index);
        (left, Some(left))
    }
}

impl Source for ChipDecoder {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.samples.len().saturating_sub(self.index))
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(1).expect("mono channel count is non-zero")
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(SAMPLE_RATE).expect("sample rate is non-zero")
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(
            self.samples.len() as f64 / SAMPLE_RATE as f64,
        ))
    }
}

impl Decodable for ChipSource {
    type Decoder = ChipDecoder;

    fn decoder(&self) -> Self::Decoder {
        ChipDecoder {
            samples: self.samples.clone(),
            index: 0,
        }
    }
}

/// 本帧待播放音效队列（由事件特效消费系统填充）。
#[derive(Resource, Default)]
pub struct SfxQueue(pub Vec<SfxId>);

/// 唯一播放入口：`Assets<ChipSource>` 未注册时静默丢弃，headless 安全。
pub fn play_sfx(
    mut commands: Commands,
    mut queue: ResMut<SfxQueue>,
    assets: Option<ResMut<Assets<ChipSource>>>,
) {
    let ids = std::mem::take(&mut queue.0);
    let Some(mut assets) = assets else {
        return;
    };
    for id in ids.into_iter().take(MAX_SFX_PER_FRAME) {
        let handle = assets.add(ChipSource::from_id(id));
        // `AudioPlayer::new` 硬编码 `AudioSource`；自定义音源必须用元组构造语法。
        commands.spawn((AudioPlayer(handle), PlaybackSettings::DESPAWN));
    }
}

/// 音频插件：注册队列与播放系统。`add_audio_source::<ChipSource>()` 由
/// 带 `DefaultPlugins` 的二进制入口调用，库级测试无需音频设备。
pub struct ChipAudioPlugin;

impl Plugin for ChipAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SfxQueue>()
            .add_systems(Update, play_sfx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sfx_table_is_well_formed() {
        use std::collections::HashSet;
        let ids: HashSet<_> = SFX_TABLE.iter().map(|d| d.id).collect();
        assert_eq!(ids.len(), 7);
        for d in &SFX_TABLE {
            assert!(d.duration > 0.0 && d.duration <= 0.5, "音效不得超过 0.5 s");
            assert!(d.gain > 0.0 && d.gain <= 1.0);
            let sum = d.attack + d.decay + d.sustain + d.release;
            assert!(
                (sum - 1.0).abs() < 1e-4,
                "ADSR 四段之和须为 1.0，得到 {sum}"
            );
            assert!(d.freq > 0.0 && d.freq_end > 0.0);
        }
    }

    #[test]
    fn envelope_starts_and_ends_at_silence() {
        let d = sfx_def(SfxId::Attack);
        assert_eq!(envelope(0.0, d), 0.0);
        assert_eq!(envelope(1.0, d), 0.0);
        assert!(envelope(0.5, d) > 0.0);
        assert!(envelope(0.5, d) <= 1.0);
    }

    #[test]
    fn synthesized_samples_are_quantized_and_bounded() {
        for d in &SFX_TABLE {
            let buf = synthesize(d);
            assert!(!buf.is_empty());
            for &s in &buf {
                assert!(s.abs() <= 1.0, "样本越界 {s}");
                let levels = s * QUANT_LEVELS;
                assert!((levels - levels.round()).abs() < 1e-4, "必须是 4-bit 量化");
            }
        }
    }

    #[test]
    fn synthesis_is_deterministic() {
        let a = synthesize(sfx_def(SfxId::Kill));
        let b = synthesize(sfx_def(SfxId::Kill));
        assert_eq!(a, b, "噪声也必须可复现（确定性 LCG）");
        assert_ne!(a, synthesize(sfx_def(SfxId::Cast)));
    }

    #[test]
    fn pcm_conversion_preserves_length_and_signal() {
        let buf = synthesize(sfx_def(SfxId::Victory));
        let pcm = pcm_i16(&buf);
        assert_eq!(pcm.len(), buf.len());
        assert_eq!(pcm.len(), (SAMPLE_RATE as f32 * 0.42) as usize);
        assert!(pcm.iter().any(|&v| v != 0), "转换后不得全静音");
        for (&s, &v) in buf.iter().zip(pcm.iter()) {
            assert_eq!(v, (s.clamp(-1.0, 1.0) * 32_767.0) as i16);
        }
    }

    #[test]
    fn event_seed_mapping_is_explicit_and_sparse() {
        assert_eq!(sfx_for_event(82), Some(SfxId::Kill));
        assert_eq!(sfx_for_event(83), Some(SfxId::TurretDown));
        assert_eq!(sfx_for_event(85), Some(SfxId::Capture));
        assert_eq!(sfx_for_event(93), Some(SfxId::Overtime));
        assert_eq!(sfx_for_event(94), Some(SfxId::Victory));
        assert_eq!(sfx_for_event(13), Some(SfxId::Cast));
        // 高频事件刻意不发声，避免刷屏
        assert_eq!(sfx_for_event(84), None);
        assert_eq!(sfx_for_event(86), None);
        assert_eq!(sfx_for_event(999), None);
    }

    #[test]
    fn chip_decoder_exhausts_and_reports_duration() {
        use bevy::audio::Source;
        let src = ChipSource::from_id(SfxId::Attack);
        let expected = src.samples.len();
        let mut dec = src.decoder();
        assert_eq!(dec.current_span_len(), Some(expected));
        assert_eq!(dec.channels().get(), 1);
        assert_eq!(dec.sample_rate().get(), SAMPLE_RATE);
        assert!(dec.total_duration().is_some());
        assert_eq!(dec.by_ref().count(), expected);
        assert_eq!(dec.current_span_len(), Some(0));
        assert!(dec.next().is_none());
    }

    /// 运行时护栏：无 `Assets<ChipSource>` 时播放系统静默降级，不 panic。
    #[test]
    fn play_sfx_is_silent_in_headless() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<SfxQueue>()
            .add_systems(Update, play_sfx);
        app.world_mut()
            .resource_mut::<SfxQueue>()
            .0
            .push(SfxId::Kill);
        app.update();
        assert!(app.world().resource::<SfxQueue>().0.is_empty());
    }
}
