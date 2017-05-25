// Copyright 2017 Adam Greig
// Licensed under the MIT license, see LICENSE for details.

#![no_std]
#![deny(missing_docs)]

//! Labrador-LDPC implements a selection of LDPC error correcting codes,
//! including encoders and decoders.
//!
//! It is designed for use with other Labrador components but does not have any dependencies
//! on anything (including `std`) and thus may be used totally standalone. It is reasonably
//! efficient on both serious computers and on small embedded systems. Considerations have
//! been made to accommodate both use cases.
//!
//! No memory allocations are made inside this crate so most methods require you to pass in
//! an allocated block of memory for them to initialise, and then later require you to pass it
//! back when it must be read. Check individual method documentation for further details.
//!
//! Please note this library is still in version 0 and so the API is likely to change.
//! In particular the current interface for passing initialised values (`g`, `cs`, etc)
//! into encoders and decoders is not ergonomic and is likely to change. On the other
//! hand the codes themselves will not change (although new ones may be added) and so newer
//! versions of the library will still be able to communicate with older versions indefinitely.
//!
//! ## Example
//!
//! ```
//! extern crate labrador_ldpc;
//! use labrador_ldpc::LDPCCode;
//!
//! fn main() {
//!     // Pick the TC128 code, n=128 k=64
//!     // (that's 8 bytes of user data encoded into 16 bytes)
//!     let code = LDPCCode::TC128;
//!
//!     // Generate some data to encode
//!     let txdata: Vec<u8> = (0..8).collect();
//!
//!     // Allocate memory for the encoded data
//!     let mut txcode = vec![0u8; code.n()/8];
//!
//!     // Encode
//!     code.encode_small(&txdata, &mut txcode);
//!
//!     // Copy the transmitted data and corrupt a few bits
//!     let mut rxcode = txcode.clone();
//!     rxcode[0] ^= 0x55;
//!
//!     // Allocate and initialise the data needed to run a decoder
//!     // (we only need ci and cs for this code and decoder, but
//!     //  normally you'd need vi and vs as well).
//!     let mut ci = vec![0; code.sparse_paritycheck_ci_len()];
//!     let mut cs = vec![0; code.sparse_paritycheck_cs_len()];
//!     code.init_sparse_paritycheck_checks(&mut ci, &mut cs);
//!
//!     // Allocate some memory for the decoder's working area and output
//!     let mut working = vec![0u8; code.decode_bf_working_len()];
//!     let mut rxdata = vec![0u8; code.output_len()];
//!
//!     // Decode
//!     code.decode_bf(&ci, &cs, None, None, &rxcode, &mut rxdata, &mut working);
//!
//!     // Check the errors got corrected
//!     assert_eq!(&rxdata[..8], &txdata[..8]);
//! }
//! ```
//!
//! ## Codes
//!
//! *Nomenclature:* we use n to represent the code length (number of bits you have to
//! transmit per codeword), k to represent the code dimension (number of useful information bits
//! per codeword), and r to represent the *rate* k/n, the number of useful information bits per
//! bit transmitted.
//!
//! Several codes are available in a range of lengths and rates. Current codes come from two
//! sets of CCSDS recommendations, their TC (telecommand) short-length low-rate codes, and their
//! TM (telemetry) higher-length various-rates codes.
//!
//! The TC codes are available in rate r=1/2 and dimensions k=128, k=256, and k=512.
//! They are the same codes defined in CCSDS document 231.1-O-1 and subsequent revisions (although
//! the n=256 code is eventually removed, it lives on here as it's quite useful).
//!
//! The TM codes are available in r=1/2, r=2/3, and r=4/5, for dimensions k=1024 and k=4096.
//! They are the same codes defined in CCSDS document 131.0-B-2 and subsequent revisions.
//!
//! For more information on the codes themselves please see the CCSDS publications:
//! https://public.ccsds.org/
//!
//! The available codes are the variants of the `LDPCCode` enum, and pretty much everything
//! else (encoders, decoders, utility methods) are implemented as methods on this enum.
//!
//! *Which code should I pick?*: for short and highly-reliable messages, the TC codes make sense,
//! especially if they need to be decoded on a constrained system such as an embedded platform.
//! For most other data transfer, the TM codes are more flexible and generally better suited.
//!
//! The very large k=16384 TM codes have not been included due to the complexity in generating
//! their generator matrices and the very long constants involved, but it would be theoretically
//! possible to include them. The relevant parity check constants are already included.
//!
//! ### Generator Matrices
//!
//! To encode a codeword, we need a generator matrix, which is a large binary matrix of shape
//! k rows by n columns. For each bit set in the data to encode, we sum the corresponding row
//! of the generator matrix to find the output codeword. Because all our codes are *systematic*,
//! the first k bits of our codewords are exactly the input data, which means we only need to
//! encode the final n-k parity bits at the end of the codeword.
//!
//! These final n-k columns of the generator are stored in a compact form, where only a small
//! number of the final rows are stored, and the rest can be generated from those at runtime.
//! This is compact to avoid wasting space in flash memory, but makes encoding data somewhat slow,
//! so there is a choice to expand to the full generator matrix instead. See the encoding methods
//! below for more details.
//!
//! The relevant constants are in the `codes.compact_generators` module, with names like `TC128_G`.
//! To expand these into a full generator matrix, you use the `LDPCCode.init_generator` method.
//!
//! It would be possible to write an encoder that did not use the generator matrix at all, not even
//! the constants, and instead used the parity check matrix to encode data. This doesn't exist yet.
//!
//! ### Parity Check Matrices
//!
//! These are the counterpart to the generator matrices of the previous section. They are used by
//! the decoders to work out which bits are wrong and need to be changed. When fully expanded,
//! they are a large matrix with n-k rows (one per parity check) of n columns (one per input data
//! bit, or variable). We can store them in an extremely compact form due to the way these specific
//! codes have been constructed, but they must be expanded before use.
//!
//! The constants are in `codes.compact_parity_checks` and reflect the construction defined
//! in the CCSDS documents. They can be expanded into the full parity matrix using the
//! `LDPCCode.init_paritycheck()` method, but this isn't actually needed by the decoders.
//!
//! Because the parity check matrices are sparse, it is much more efficient to store them as arrays
//! of the indices of non-zero entries. For each check (row), we store the column indices that are
//! non-zero in the array `ci` (*c*heck *i*ndex), and we store indices into `ci` in the array `cs`
//! (*c*heck *s*tarts). In other words, for check 100, the relevant variables are stored in
//! `ci[cs[100]..cs[101]]`. We then do the same from the other direction, storing the non-zero rows
//! for each variable in the `vi` and `vs` arrays. These arrays are initialised using the
//! `LDPCCode.init_sparse_paritycheck()` method, or you can initialise just the `ci` and `cs`
//! arrays using `LDPCCode.init_sparse_paritycheck_checks()` (which is useful for the `bf` decoder
//! with the `TC` codes, where you don't need `vi` or `vs`).
//!
//! It would be possible to write a decoder that directly accessed the parity constants, removing
//! the need for any expansion into RAM, but this doesn't exist yet.
//!
//! ## Encoders
//!
//! There are two encoders available. Both of them take a `&[u8]` of input data and encode it
//! to a `&mut [u8]`.
//!
//! * The low-memory encoder, `encode_small`, which is as much as a hundred times slower but
//!   does not need to have the generator matrix in RAM at all, and so has a low memory overhead.
//!   The only memory required is that needed for the input and output codewords in a compact
//!   binary representation (i.e. one u8 per eight bits of data).
//!
//! * The fast encoder, `encode_fast`, is much quicker, but requires the generator matrix `g`
//!   to have been initialised at runtime by the `LDPCCode.init_generator()` method.
//!   The precise overhead depends on the code in use, and is available as a constant
//!   `CodeParams.generator_len` or at runtime as `LDPCCode.generator_len()` (it's the length of a
//!   u32 array, rather than the number of bytes), in addition to the memory required for the input
//!   and output data.
//!
//! The required memory (in bytes) to encode with each code is:
//!
//! Code   | Input       | Output          | Generator const | Expanded generator
//! -------|-------------|-----------------|-----------------|---------------------------
//!        | (Data, RAM) | (Codeword, RAM) | (text)          | (`encode_fast` only, RAM)
//!        | =k/8        | =n/8            |                 | =k*(n-k)/8
//! TC128  |           8 |              16 |              32 |                       512
//! TC256  |          16 |              32 |              64 |                      2048
//! TC512  |          32 |              64 |             128 |                      8192
//! TM1280 |         128 |             160 |            1024 |                     32768
//! TM1536 |         128 |             192 |            1024 |                     65536
//! TM2048 |         128 |             256 |            1024 |                    131072
//! TM5120 |         512 |             640 |            4096 |                    524288
//! TM6144 |         512 |             768 |            4096 |                   1048576
//! TM8192 |         512 |            1024 |            4096 |                   2097152
//!
//! Both encoders require the same input data and output codeword storage, and the same generator
//! constants. Only the `encode_fast` encoder requires the additional overhead of the expanded
//! generator in RAM. These constants (expressed as lengths of Vecs of the appropriate type)
//! are available both at compile-time in the `CodeParams` consts, and at runtime with methods
//! on `LDPCCode` such as `generator_len()`. You can therefore allocate the required memory either
//! statically or dynamically at runtime.
//!
//! ## Decoders
//!
//! There are two decoders available:
//!
//! * The low-memory decoder, `decode_bf`, uses a bit flipping algorithm with hard information.
//!   This is maybe 1 or 2dB from optimal for decoding, but requires much less RAM and does still
//!   correct errors. It's only really useful on something without a hardware floating point unit
//!   or with very little memory available.
//! * The high-performance decoder, `decode_mp`, uses a modified min-sum decoding algorithm to
//!   perform near-optimal decoding albeit with much higher memory overhead.
//!
//! The required memory (in bytes) to decode with each code is:
//!
//! Code   | Hard input  | Soft input  |   Output | Parity const | `bf` overhead | `mp` overhead
//! -------|-------------|-------------|----------|--------------|---------------|--------------
//!        | (`bf`, RAM) | (`mp`, RAM) | (RAM)    | (text)       | (RAM)         | (RAM)
//!        | =n/8        | =n*4        | =(n+p)/8 |              |               |
//! TC128  |          16 |         512 |       16 |           32 |          1282 |         6532
//! TC256  |          32 |        1024 |       32 |           32 |          2562 |        13060
//! TC512  |          64 |        2048 |       64 |           32 |          5122 |        26116
//! TM1280 |         160 |        5120 |      176 |          369 |         24964 |        63492
//! TM1536 |         192 |        6144 |      224 |          324 |         30468 |        75780
//! TM2048 |         256 |        8192 |      320 |          279 |         41476 |       100356
//! TM5120 |         640 |       20480 |      704 |          369 |         99844 |       253956
//! TM6144 |         768 |       24576 |      896 |          324 |        121860 |       303108
//! TM8192 |        1024 |       32768 |     1280 |          279 |        165892 |       401412
//!
//! The overhead column gives the combination of the required expanded parity constants (`ci`,
//! `cs`, `vi`, and `vs`), and the required working area for that decoder. The `bf` decoder with
//! the `TC` codes only requires the `ci` and `cs` parity data, giving the smallest memory
//! overhead.
//!
//! Both decoders require the same output storage and parity constants. The `bf` decoder takes
//! smaller hard inputs and has a much smaller working area, while the `mp` decoder requires
//! soft inputs (one f32 per input bit) and uses soft information internally, requiring a larger
//! working area.
//!
//! The required sizes are available both at compile-time in the `CodeParams` consts, and at
//! runtime with methods on `LDPCCode` such as `sparse_paritycheck_cs_len()`. You can therefore
//! allocate the required memory either statically or dynamically at runtime.
//!
//! Please see the individual decoder methods for more details on their requirements.
//!
//! ### Bit Flipping Decoder
//! This decoder is based on the original Gallager decoder. It is not very optimal but is very
//! fast. The idea is to see which bits are connected to the highest number of parity checks
//! that are not currently satisfied, and flip those bits, and iterate until things get better.
//! However, this routine cannot correct erasures (it only knows about bit flips). All of the TM
//! codes are *punctured*, which means some parity bits are not transmitted and so are unknown
//! at the receiver. We use a separate algorithm to decode the erasures first, based on a paper
//! by Archonta, Kanistras and Paliouras, doi:10.1109/MOCAST.2016.7495161.
//!
//! ### Message Passing Decoder
//! This is a modified min-sum decoder that computes the probability of each bit being set given
//! the other bits connected to it via the parity check matrix. It takes soft information in,
//! so naturally covers the punctured codes as well. This implementation is based on one described
//! by Savin, arXiv:0803.1090. It is both reasonably efficient (no `atahn` required), and
//! performs very close to optimal sum-product decoding. Error performance could possibly be
//! improved using a min-sum correction factor for our codes, and decoding speed could be improved
//! by constructing a set of inverse lookup tables for the parity checks, at the expense of
//! significantly increased memory use.
//!
//! A fixed point version of this decoder would be useful, as it could potentially be optimised for
//! embedded DSP operation. 8 bits per symbol seems plenty of resolution for the soft information,
//! so error performance could be unaffected while gaining very substantial speed and memory
//! improvements.
//!

#[cfg(test)]
#[macro_use]
extern crate std;

pub mod codes;
pub mod encoder;
pub mod decoder;
pub use codes::{LDPCCode};
