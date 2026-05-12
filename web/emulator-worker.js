// emulator-worker.js — Phase B Step 1
//
// Runs the SNES emulator off the main thread. Communicates with the page
// purely via postMessage. No SAB yet (that's Step 2), no AudioWorklet
// (that's Step 3). Audio samples are piggybacked on the per-frame message
// for now; the main thread will play them through its existing audio
// path, which will glitch under load — that's expected at this stage.
//
// Loaded as an ES-module worker:  new Worker(url, { type: 'module' })

import init, { Emulator } from './pkg/zelda_a_link_to_the_past.js';

let emulator = null;
let wasmMemory = null;     // captured from init() return value
let loopHandle = null;
let frameSeq = 0;

// Drive at NTSC 60.0988 Hz. setInterval is coarse but adequate as a
// scaffold; a future revision will key the cadence to AudioContext clock
// once audio is on a worklet (Step 3).
const FRAME_MS = 1000 / 60.0988;

async function handleLoad(romBytes) {
    const wasm = await init();
    // wasm-bindgen exposes the WebAssembly.Memory on the init() result.
    wasmMemory = wasm.memory;
    emulator = new Emulator(romBytes);
    self.postMessage({ type: 'ready' });
}

function tick() {
    if (!emulator) return;

    emulator.run_frame_no_return();
    frameSeq++;

    // Zero-copy view into WASM memory, then copy into a fresh buffer
    // we can transfer. (We can't transfer WASM memory itself.)
    const fbLen = emulator.framebuffer_len();
    const fbPtr = emulator.framebuffer_ptr();
    const fbView = new Uint8Array(wasmMemory.buffer, fbPtr, fbLen);
    const fbCopy = new Uint8Array(fbLen);
    fbCopy.set(fbView);

    // Drain audio samples — Int16Array, interleaved stereo.
    // get_audio_samples copies + clears internally on the Rust side.
    const audio = emulator.get_audio_samples();
    const audioCopy = new Int16Array(audio.length);
    audioCopy.set(audio);

    self.postMessage(
        {
            type: 'frame',
            seq: frameSeq,
            frameCount: emulator.frame_count(),
            fb: fbCopy,
            audio: audioCopy,
        },
        [fbCopy.buffer, audioCopy.buffer]
    );
}

function startLoop() {
    if (loopHandle !== null || !emulator) return;
    loopHandle = setInterval(tick, FRAME_MS);
}

function stopLoop() {
    if (loopHandle !== null) {
        clearInterval(loopHandle);
        loopHandle = null;
    }
}

self.onmessage = async (ev) => {
    const msg = ev.data;
    switch (msg.type) {
        case 'load':
            await handleLoad(msg.rom);
            break;
        case 'start':
            startLoop();
            break;
        case 'stop':
            stopLoop();
            break;
        case 'input':
            if (emulator) emulator.set_button(msg.button, msg.pressed);
            break;
        default:
            // Unknown message — ignore.
            break;
    }
};
