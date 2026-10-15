//! Generic GPU providers extracted from app crates over time (craft-libs#7).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod composite;
mod echo;
mod lut;
mod mask;
mod noop;
mod params;
mod readback;
mod transition;

#[cfg(test)]
mod tests;

use craft_gpu::{GpuProvider, ProviderRegistry};

pub use composite::PremulCompositeProvider;
pub use echo::EchoProvider;
pub use lut::LutProvider;
pub use mask::MaskProvider;
pub use noop::NoopProvider;
pub use readback::ReadbackProvider;
pub use transition::PushTransitionProvider;

/// Default shared providers for worker/runtime init (`craft.*` ids only).
pub fn register_all() -> Vec<Box<dyn GpuProvider>> {
    vec![
        Box::new(NoopProvider),
        Box::new(EchoProvider),
        Box::new(MaskProvider),
        Box::new(PremulCompositeProvider),
        Box::new(ReadbackProvider),
        Box::new(LutProvider),
        Box::new(PushTransitionProvider),
    ]
}

/// Register [`register_all`] providers into an existing registry.
pub fn register_all_into(registry: &mut ProviderRegistry) {
    for provider in register_all() {
        registry.register(provider);
    }
}
