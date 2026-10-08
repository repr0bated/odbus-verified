# Official libfuse Reference

Official sources:
- GitHub repository: `https://github.com/libfuse/libfuse`
- API documentation: `https://libfuse.github.io/doxygen`

What the official repo provides:
- `example/`: starting points for real filesystems and callback wiring
- `include/fuse.h`: high-level API
- `include/fuse_lowlevel.h`: low-level API
- `README.md`: build, install, and security notes
- `util/fusermount3`: mount helper used in common setups

Build notes from the official project:
- libfuse itself is built with Meson and Ninja
- normal downstream projects usually link with `pkg-config fuse3 --cflags --libs`

Key operational constraints from libfuse:
- `fusermount3` is commonly setuid root in normal installations
- users can only mount where they have write permission
- `allow_other` is controlled through `/etc/fuse.conf`
- `default_permissions` matters for correct kernel-side permission enforcement

When reviewing or implementing code:
- prefer adapting an official example over writing a callback table from memory
- match the task to the correct API layer first
- for low-level API, verify every request path sends an explicit reply
