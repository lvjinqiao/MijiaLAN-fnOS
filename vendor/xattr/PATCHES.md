# Local compatibility patch

Source: crates.io `xattr` 0.2.3, checksum
`6d1526bbe5aaeb5eb06885f4d987bcdfa5e23187055de9b83fe00156a821fabc`.

`src/sys/mod.rs` uses `libc::ENODATA` on Linux and Android for missing
extended attributes. New libc versions removed the Linux `ENOATTR` alias.
The crate's existing API, other platforms, and error handling are unchanged.
Both original license files are retained.
