// The real-time render entry of the phone player (src/player.rs). Not part of the UniFFI
// interface: the platform's audio callback calls it directly (no allocation, no lock).
#pragma once
#include <stdint.h>

// Fills `frames` interleaved frames of `channels` f32 samples; returns the frames taken from
// the track (the rest is silence). `handle` comes from `Player.takeRenderHandle()`;
// `host_time` — the host time the first frame sounds at (`AudioTimeStamp.mHostTime`; 0 — unknown):
// a transition starts the next track by it, to the sample (D-167).
uint32_t sirtone_render(uint64_t handle, float *out, uint32_t frames, uint32_t channels,
                        uint64_t host_time);

// Frees a render handle once the audio node that used it is detached.
void sirtone_render_release(uint64_t handle);
