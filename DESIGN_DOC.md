# Firewheel Design Document

# Overview

The Rust ecoystem (and the libre game dev ecosystem as a whole) is in need of a powerful, flexible, and libre audio engine for games. game-audio-engine aims to provide developers with a flexible, easy-to-use framework for constructing custom audio experiences.

# Goals

* Flexible audio graph engine (supports any directed, acyclic graph with support for one-to-many connections)
* Cycle detection for invalid audio graphs
* A suite of essential built-in nodes:
    * gain (minimum value mutes)
    * stereo panning
    * stereo width
    * sum
    * filters (lowpass, highpass, bandpass)
    * echo
    * delay compensation
    * hard clip
    * convolutional reverb
    * spatial positioning (make a sound "emenate" from a point in 3d space)
    * sampler
        * gain envelope
        * looping
        * doppler stretching
        * disk and network streaming
    * test tone
    * decibel meter
* Support for custom audio nodes to allow for a plethora of 3rd party generators and effects
* Basic CLAP plugin hosting (non-WASM only)
* Automatable parameters on nodes, with support for bezier automation curves
* Support for loading a wide variety of audio formats (using Symphonia)
* Modular backend (the engine can run on top of any audio stream), with default backends including CPAL and RtAudio.
* Fault tolerance for audio streams (The game shouldn't crash just because the player accidentally unplugged their headphones.)
* Silence optimizations (avoid processing if the audio buffer contains all zeros, useful when using "pools" of nodes where the majority of the time nodes are unused.)
* An API that plays nicely with the Bevy game engine
* Properly respect realtime constraints (no mutexes!)
* WASM support

# Non-Goals

* MIDI on the audio-graph level (It will still be possible to create a custom sampler/synthesizer that reads a MIDI file as input.)
* Parameter events on the audio-graph level (as in you can't pass parameter events from one node to another)
* Connecting to system MIDI devices
* Built-in synthesizers (This can be done with third-party nodes/CLAP plugins.)
* Advanced mixing effects like parametric EQs, compressors, and limiters (This again can be done with third-party nodes/CLAP plugins.)
* GUIs for hosted CLAP plugins (This is a game audio engine, not a DAW audio engine.)
* Multi-threaded audio graph processing (This would make the engine a lot more complicated, and it is probably overkill for games.)
* VST, VST3, LV2, and AU plugin hosting

# Possible Goals

* Advanced pitch/time stretching built in to the sampler node (rubberband library?)
* C bindings for use in other languages

# API

TODO