# Third-Party Software Notices

This file records software that is used by MomoBako but is not covered by any license grant for MomoBako's own source code. Each component remains subject to its own license.

## FFmpeg command-line tools

MomoBako invokes the external `ffmpeg` and `ffprobe` command-line programs through the `ffmpeg-sidecar` Rust crate. When no compatible local installation is found, the current implementation may download a platform-specific FFmpeg build at runtime.

FFmpeg is a separate program. Its effective license depends on the configuration of the downloaded build. Builds that enable GPL components such as x264 or x265 are distributed under the GNU General Public License, commonly GPL-3.0-or-later. Other builds may use the LGPL. MomoBako's own license does not sublicense or replace the license of the downloaded FFmpeg programs.

A distributor that bundles, mirrors, preinstalls, or otherwise conveys an FFmpeg build with MomoBako must:

- identify the exact FFmpeg version, provider, download URL, configuration, and binary hash;
- preserve the copyright and license output reported by `ffmpeg -L`;
- include the applicable GPL/LGPL license text and third-party notices;
- provide the corresponding source code, or a compliant source offer, for the exact conveyed build when required;
- ensure that product terms do not restrict rights granted for the FFmpeg component, including rights needed to modify or replace it.

The license reported by the actual downloaded binary is authoritative. Do not describe a MomoBako distribution as entirely MIT-licensed, proprietary, or otherwise uniformly licensed when it contains an FFmpeg binary under different terms.

## ffmpeg-sidecar

`ffmpeg-sidecar` is an independent Rust wrapper used to locate, download, and launch FFmpeg command-line programs. It is distributed under the MIT License. Its MIT license does not change the license of FFmpeg itself.

## Other dependencies

Cargo and JavaScript dependencies retain the licenses declared by their respective authors. A release process should generate and ship a version-specific dependency manifest in addition to this manually maintained notice.
