// audio-worklet-processor.js — SNES AudioWorklet (Phase B Step 3 / T13)
//
// Runs on the audio rendering thread, completely decoupled from the main
// thread and the emulator worker. Reads i16 stereo samples from a
// SharedArrayBuffer ring buffer and converts them to float output.
//
// Implements Near/byuu's Dynamic Rate Control (DRC): continuously adjusts
// how many source samples to consume per output quantum to keep the buffer
// approximately half-full, preventing both underflow (pops) and overflow
// (growing latency).
//
// Ring buffer layout (SharedArrayBuffer):
//   bytes [0..3]    write_pos: Uint32 (Atomics, updated by worker)
//   bytes [4..7]    read_pos:  Uint32 (Atomics, updated by this processor)
//   bytes [8..]     samples:   Int16[] (stereo interleaved: L, R, L, R, ...)
//
// The ring capacity is (SAB.byteLength - 8) / 2 samples total,
// i.e., capacity/2 stereo frames.

class SNESAudioProcessor extends AudioWorkletProcessor {
    constructor() {
        super();

        this.ring = null;        // Int16Array view of sample data
        this.controlU32 = null;  // Uint32Array view of write_pos / read_pos
        this.capacity = 0;       // total i16 samples in ring
        this.ready = false;

        // DRC state
        this.srcRate = 32040;    // real SNES APU frequency (NOT 32000)
        this.dstRate = sampleRate; // AudioContext.sampleRate (usually 48000)
        this.resamplePos = 0;    // fractional position in source stream

        // Audio gain (matches Phase A: ×4)
        this.gain = 4.0;

        // DRC parameters (Near's formula)
        this.maxDelta = 0.005;   // 0.5% max pitch distortion

        this.port.onmessage = (ev) => {
            if (ev.data.type === 'init') {
                const sab = ev.data.sab;
                this.controlU32 = new Uint32Array(sab, 0, 2);
                this.ring = new Int16Array(sab, 8);
                this.capacity = this.ring.length;
                this.ready = true;
            }
        };
    }

    process(inputs, outputs, parameters) {
        if (!this.ready) return true;

        const outL = outputs[0][0];
        const outR = outputs[0][1];
        if (!outL || !outR) return true;

        const writePos = Atomics.load(this.controlU32, 0);
        const readPos = Atomics.load(this.controlU32, 1);

        // Samples available in the ring (stereo i16 values, so /2 for frames)
        const available = (writePos - readPos + this.capacity) % this.capacity;
        const stereoFramesAvailable = (available >> 1); // pairs

        // ── Dynamic Rate Control (Near/byuu) ──────────────────────
        //
        // fillLevel = fraction of buffer capacity currently filled (0..1)
        // dynamicRate = adjusted source sample rate that keeps buffer ~50% full
        //
        // When buffer is >50% full, we consume faster (dynamicRate increases).
        // When buffer is <50% full, we consume slower (dynamicRate decreases).
        // Maximum pitch distortion is ±maxDelta (0.5%), inaudible.
        const bufferCapacity = this.capacity >> 1; // stereo frames
        const fillLevel = stereoFramesAvailable / bufferCapacity;
        const dynamicRate = ((1.0 - this.maxDelta) + 2.0 * fillLevel * this.maxDelta)
                            * this.srcRate;

        // Resample ratio: how many source frames per output frame
        const ratio = dynamicRate / this.dstRate;

        let localReadPos = readPos;
        const cap = this.capacity;

        for (let i = 0; i < outL.length; i++) {
            // Integer index into the source ring (stereo frame index)
            const srcIdx = Math.floor(this.resamplePos);
            const frac = this.resamplePos - srcIdx;

            // Read two consecutive stereo frames for linear interpolation
            const idx0 = (localReadPos + srcIdx * 2) % cap;
            const idx1 = (localReadPos + srcIdx * 2 + 2) % cap;

            if (srcIdx + 1 < stereoFramesAvailable) {
                // Linear interpolation between adjacent samples
                const l0 = this.ring[idx0]     / 32768.0;
                const r0 = this.ring[idx0 + 1] / 32768.0;
                const l1 = this.ring[idx1]     / 32768.0;
                const r1 = this.ring[idx1 + 1] / 32768.0;

                outL[i] = Math.max(-1, Math.min(1, (l0 + frac * (l1 - l0)) * this.gain));
                outR[i] = Math.max(-1, Math.min(1, (r0 + frac * (r1 - r0)) * this.gain));
                this.resamplePos += ratio;
            } else {
                // Underrun: output silence, do NOT advance resamplePos.
                // Advancing past valid data causes the read pointer to lap
                // the write pointer — the ring then reports its entire
                // capacity as "available", replaying stale audio garbage.
                outL[i] = 0;
                outR[i] = 0;
            }
        }

        // Advance read position by the number of source frames consumed
        const consumed = Math.floor(this.resamplePos);
        localReadPos = (localReadPos + consumed * 2) % cap;
        this.resamplePos -= consumed;

        Atomics.store(this.controlU32, 1, localReadPos);

        return true;
    }
}

registerProcessor('snes-audio-processor', SNESAudioProcessor);
