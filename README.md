# Firewheel

[![Documentation](https://docs.rs/firewheel/badge.svg)](https://docs.rs/firewheel)
[![Crates.io](https://img.shields.io/crates/v/firewheel.svg)](https://crates.io/crates/firewheel)
[![License](https://img.shields.io/crates/l/firewheel.svg)](https://github.com/BillyDM/firewheel/blob/main/LICENSE)

*Work In Progress*

Firewheel is a flexible, high-performance, and libre audio engine for games.

## Key Features
* Flexible audio graph engine (supports any directed, acyclic graph with support for one-to-many connections)
* A suite of built-in nodes for common tasks:
    * gain, stereo panning, stereo width (TODO)
    * summation (TODO)
    * versatile sample player (TODO)
        * disk/network streaming (TODO)
    * effects like filters, delays (echos), clippers, and convolutional reverbs (TODO)
    * spatial positioning (make a sound "emanate" from a point in 3d space) (TODO)
    * Decibel meter (TODO)
* Custom audio node API allowing for a plethora of 3rd party generators and effects
* Basic [CLAP](https://cleveraudio.org/) plugin hosting (non-WASM only) (TODO)
* Automatable parameters on nodes, with support for automation curves (TODO)
* Fault tolerance for audio streams (The game shouldn't crash just because the player accidentally unplugged their headphones.)
* WASM support (TODO)
* C bindings (TODO)

## Motivation

Firewheel is intended to be the default audio engine powering the [Bevy](https://bevyengine.org/) game engine. Firewheel was separated out of Bevy into its own repository in the hopes that it could be useful for other games and game engines.

## Get Involved

Join the discussion in the [Bevy Discord Server](https://discord.gg/bevy) under the `working-groups -> Better Audio` channel!