//! The relocation kinds of the chips plugins are made for, by what they
//! mean when the image moves (numbers as in binutils' `elf/riscv.h` and
//! `elf/xtensa.h`).

use paperpaw_native::Chip;

/// What a relocation kind does, for an image that moves as one block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Changes nothing (relaxation and alignment hints).
    Marker,
    /// A whole 32-bit word that holds an address.
    Word,
    /// An address in an instruction, or in part of a word: right only
    /// where it was linked.
    Absolute,
    /// A distance from where it lies: right wherever the image is, if
    /// what it reaches moves with it.
    Relative,
    /// A difference of two places (or one of its two halves): right if
    /// both move with the image.
    Difference,
    /// A kind this tool does not know.
    Unknown,
}

/// What relocation kind `number` of `chip` does.
pub fn kind(chip: Chip, number: u32) -> Kind {
    match chip {
        Chip::Esp32C3 => match number {
            // NONE, ALIGN, RELAX
            0 | 43 | 51 => Kind::Marker,
            // 32
            1 => Kind::Word,
            // HI20, LO12_I, LO12_S, RVC_LUI
            26..=28 | 46 => Kind::Absolute,
            // BRANCH, JAL, CALL, CALL_PLT, PCREL_HI20, PCREL_LO12_I,
            // PCREL_LO12_S, RVC_BRANCH, RVC_JUMP, 32_PCREL, PLT32
            16..=19 | 23..=25 | 44 | 45 | 57 | 59 => Kind::Relative,
            // ADD8..ADD64, SUB8..SUB64, SUB6, SET6, SET8, SET16, SET32,
            // SET_ULEB128, SUB_ULEB128
            33..=40 | 52..=56 | 60 | 61 => Kind::Difference,
            _ => Kind::Unknown,
        },
        Chip::Esp32S3 => match number {
            // NONE, ASM_EXPAND, ASM_SIMPLIFY
            0 | 11 | 12 => Kind::Marker,
            // 32
            1 => Kind::Word,
            // SLOT0_ALT..SLOT14_ALT
            35..=49 => Kind::Absolute,
            // 32_PCREL, SLOT0_OP..SLOT14_OP (calls, jumps, branches, l32r)
            14 | 20..=34 => Kind::Relative,
            // DIFF8, DIFF16, DIFF32, PDIFF8..PDIFF32, NDIFF8..NDIFF32
            17..=19 | 57..=62 => Kind::Difference,
            _ => Kind::Unknown,
        },
        _ => Kind::Unknown,
    }
}

/// The name of relocation kind `number` of `chip`, as binutils gives it.
pub fn name(chip: Chip, number: u32) -> String {
    let known = match chip {
        Chip::Esp32C3 => match number {
            0 => "NONE",
            1 => "32",
            16 => "BRANCH",
            17 => "JAL",
            18 => "CALL",
            19 => "CALL_PLT",
            23 => "PCREL_HI20",
            24 => "PCREL_LO12_I",
            25 => "PCREL_LO12_S",
            26 => "HI20",
            27 => "LO12_I",
            28 => "LO12_S",
            33 => "ADD8",
            34 => "ADD16",
            35 => "ADD32",
            36 => "ADD64",
            37 => "SUB8",
            38 => "SUB16",
            39 => "SUB32",
            40 => "SUB64",
            43 => "ALIGN",
            44 => "RVC_BRANCH",
            45 => "RVC_JUMP",
            46 => "RVC_LUI",
            51 => "RELAX",
            52 => "SUB6",
            53 => "SET6",
            54 => "SET8",
            55 => "SET16",
            56 => "SET32",
            57 => "32_PCREL",
            59 => "PLT32",
            60 => "SET_ULEB128",
            61 => "SUB_ULEB128",
            _ => return format!("relocation kind {number}"),
        },
        Chip::Esp32S3 => match number {
            0 => "NONE",
            1 => "32",
            11 => "ASM_EXPAND",
            12 => "ASM_SIMPLIFY",
            14 => "32_PCREL",
            17 => "DIFF8",
            18 => "DIFF16",
            19 => "DIFF32",
            20..=34 => return format!("R_XTENSA_SLOT{}_OP", number - 20),
            35..=49 => return format!("R_XTENSA_SLOT{}_ALT", number - 35),
            57 => "PDIFF8",
            58 => "PDIFF16",
            59 => "PDIFF32",
            60 => "NDIFF8",
            61 => "NDIFF16",
            62 => "NDIFF32",
            _ => return format!("relocation kind {number}"),
        },
        _ => return format!("relocation kind {number}"),
    };
    let prefix = match chip {
        Chip::Esp32C3 => "R_RISCV_",
        Chip::Esp32S3 => "R_XTENSA_",
        _ => "R_",
    };
    format!("{prefix}{known}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_known_has_a_name() {
        for chip in [Chip::Esp32C3, Chip::Esp32S3] {
            for number in 0..256 {
                let known = !name(chip, number).starts_with("relocation kind");
                assert_eq!(
                    known,
                    kind(chip, number) != Kind::Unknown,
                    "{chip} {number}"
                );
            }
        }
        assert_eq!(name(Chip::Esp32C3, 26), "R_RISCV_HI20");
        assert_eq!(name(Chip::Esp32S3, 20), "R_XTENSA_SLOT0_OP");
        assert_eq!(name(Chip::Esp32S3, 49), "R_XTENSA_SLOT14_ALT");
        assert_eq!(name(Chip::Esp32S3, 99), "relocation kind 99");
    }
}
