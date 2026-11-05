// The real-time render entry of the phone player (src/player.rs). Not part of the UniFFI
// interface: the platform's audio callback calls it directly (no allocation, no lock).
#pragma once
#include <stdint.h>

// Fills `frames` interleaved frames of `channels` f32 samples; returns the frames taken from
// the track (the rest is silence). `handle` comes from `Player.takeRenderHandle()`.
uint32_t sirtone_render(uint64_t handle, float *out, uint32_t frames, uint32_t channels);

// Frees a render handle once the audio node that used it is detached.
void sirtone_render_release(uint64_t handle);
