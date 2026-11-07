//! A small reader of 32-bit little-endian ELF files (the chips' objects
//! and linked plugins): their sections, symbols and relocations with
//! addends, all bounds checked.

/// RISC-V, the ESP32-C3's.
pub const EM_RISCV: u16 = 243;
/// Xtensa, the ESP32-S3's.
pub const EM_XTENSA: u16 = 94;
/// An object file, as in an rlib.
pub const ET_REL: u16 = 1;
/// A linked file.
pub const ET_EXEC: u16 = 2;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_RELA: u32 = 4;
pub const SHT_NOBITS: u32 = 8;
pub const SHT_REL: u32 = 9;
pub const SHF_WRITE: u32 = 1;
pub const SHF_ALLOC: u32 = 2;
/// A section with machine instructions.
pub const SHF_EXECINSTR: u32 = 4;
pub const SHN_UNDEF: u16 = 0;
/// The first of the section indices with a meaning of their own.
pub const SHN_LORESERVE: u16 = 0xff00;
pub const SHN_ABS: u16 = 0xfff1;
pub const SHN_COMMON: u16 = 0xfff2;
pub const STT_SECTION: u8 = 3;

const EHDR_LEN: usize = 52;
const SHDR_LEN: usize = 40;
const SYM_LEN: usize = 16;
const RELA_LEN: usize = 12;

/// An ELF file read.
#[derive(Debug)]
pub struct Elf<'a> {
    bytes: &'a [u8],
    /// `ET_REL` or `ET_EXEC`, for instance.
    pub kind: u16,
    pub machine: u16,
    /// All sections, by their index (the first the empty one).
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub kind: u32,
    pub flags: u32,
    pub addr: u32,
    pub offset: u32,
    pub size: u32,
    pub link: u32,
    pub info: u32,
    pub addralign: u32,
}

impl Section {
    /// Whether it takes memory when the program runs.
    pub fn is_alloc(&self) -> bool {
        self.flags & SHF_ALLOC != 0
    }

    /// Whether it takes memory that is written to.
    pub fn is_writable(&self) -> bool {
        self.flags & (SHF_WRITE | SHF_ALLOC) == SHF_WRITE | SHF_ALLOC
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub value: u32,
    pub size: u32,
    pub info: u8,
    /// The section it is defined in, or `SHN_UNDEF`, `SHN_ABS`, ...
    pub shndx: u16,
}

impl Symbol {
    /// Its type (`STT_SECTION`, for instance).
    pub fn kind(&self) -> u8 {
        self.info & 15
    }
}

/// A relocation with its addend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rela {
    /// Where it applies: an address in a linked file, an offset into the
    /// section in an object file.
    pub offset: u32,
    /// The symbol's index in the symbol table (0 for none).
    pub symbol: u32,
    pub kind: u32,
    pub addend: i32,
}

/// Whether `bytes` begin as an ELF file does.
pub fn is_elf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x7fELF")
}

impl<'a> Elf<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        if !is_elf(bytes) {
            return Err("not an ELF file".into());
        }
        if bytes.len() < EHDR_LEN {
            return Err("the ELF file is cut short".into());
        }
        if bytes[4] != 1 || bytes[5] != 1 {
            return Err("not a 32-bit little-endian ELF file".into());
        }
        let shoff = le32(bytes, 32) as usize;
        let shentsize = usize::from(le16(bytes, 46));
        let shnum = usize::from(le16(bytes, 48));
        let shstrndx = usize::from(le16(bytes, 50));
        if shnum == 0 && shoff != 0 {
            return Err("too many sections for this reader".into());
        }
        if shentsize != SHDR_LEN && shnum > 0 {
            return Err("its section headers are not of ELF32's size".into());
        }
        let table = shoff
            .checked_add(shnum * SHDR_LEN)
            .and_then(|end| bytes.get(shoff..end))
            .ok_or("its section headers lie outside the file")?;
        let mut sections: Vec<Section> = table
            .as_chunks::<SHDR_LEN>()
            .0
            .iter()
            .map(|h| Section {
                name: String::new(),
                kind: le32(h, 4),
                flags: le32(h, 8),
                addr: le32(h, 12),
                offset: le32(h, 16),
                size: le32(h, 20),
                link: le32(h, 24),
                info: le32(h, 28),
                addralign: le32(h, 32),
            })
            .collect();
        let mut elf = Elf {
            bytes,
            kind: le16(bytes, 16),
            machine: le16(bytes, 18),
            sections: Vec::new(),
        };
        if shnum > 0 {
            let names = sections
                .get(shstrndx)
                .ok_or("its section names lie in no section")?;
            let names = elf.data(names)?;
            for (section, header) in sections
                .iter_mut()
                .zip(table.as_chunks::<SHDR_LEN>().0.iter())
            {
                section.name = string(names, le32(header, 0))?;
            }
        }
        elf.sections = sections;
        Ok(elf)
    }

    /// The section called `name` and its index.
    pub fn section(&self, name: &str) -> Option<(usize, &Section)> {
        self.sections
            .iter()
            .enumerate()
            .find(|(_, s)| s.name == name)
    }

    /// A section's bytes in the file (none for `SHT_NOBITS`).
    pub fn data(&self, section: &Section) -> Result<&'a [u8], String> {
        if section.kind == SHT_NOBITS {
            return Ok(&[]);
        }
        let start = section.offset as usize;
        start
            .checked_add(section.size as usize)
            .and_then(|end| self.bytes.get(start..end))
            .ok_or_else(|| format!("section {} lies outside the file", section.name))
    }

    /// The symbol table's index and its symbols (the first the empty
    /// one); none if the file has no symbol table.
    pub fn symbols(&self) -> Result<Option<(usize, Vec<Symbol>)>, String> {
        let Some((index, table)) = self
            .sections
            .iter()
            .enumerate()
            .find(|(_, s)| s.kind == SHT_SYMTAB)
        else {
            return Ok(None);
        };
        let names = self
            .sections
            .get(table.link as usize)
            .ok_or("the symbol table's names lie in no section")?;
        let names = self.data(names)?;
        let symbols = self
            .data(table)?
            .as_chunks::<SYM_LEN>()
            .0
            .iter()
            .map(|s| {
                Ok(Symbol {
                    name: string(names, le32(s, 0))?,
                    value: le32(s, 4),
                    size: le32(s, 8),
                    info: s[12],
                    shndx: le16(s, 14),
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Some((index, symbols)))
    }

    /// The relocations of a `SHT_RELA` section.
    pub fn relas(&self, section: &Section) -> Result<Vec<Rela>, String> {
        Ok(self
            .data(section)?
            .as_chunks::<RELA_LEN>()
            .0
            .iter()
            .map(|r| {
                let info = le32(r, 4);
                Rela {
                    offset: le32(r, 0),
                    symbol: info >> 8,
                    kind: info & 0xff,
                    addend: le32(r, 8) as i32,
                }
            })
            .collect())
    }
}

/// The string at `at` in a string table.
fn string(table: &[u8], at: u32) -> Result<String, String> {
    let rest = table
        .get(at as usize..)
        .ok_or("a name lies outside its table")?;
    let end = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or("a name has no end")?;
    Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
}

pub fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

pub fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Writes small ELF files for the tests.
#[cfg(test)]
pub mod build {
    use super::*;

    /// A section to write.
    #[derive(Debug, Clone)]
    struct Spec {
        name: String,
        kind: u32,
        flags: u32,
        addr: u32,
        data: Vec<u8>,
        /// For `SHT_NOBITS`: its size, as it has no bytes.
        size: u32,
        align: u32,
    }

    /// An ELF file to write: sections, then their relocations, the symbol
    /// table and the string tables, which it makes itself.
    #[derive(Debug, Clone)]
    pub struct Builder {
        machine: u16,
        kind: u16,
        sections: Vec<Spec>,
        symbols: Vec<Symbol>,
        relas: Vec<(usize, Vec<Rela>)>,
    }

    impl Builder {
        pub fn new(machine: u16, kind: u16) -> Self {
            Self {
                machine,
                kind,
                sections: Vec::new(),
                symbols: Vec::new(),
                relas: Vec::new(),
            }
        }

        /// Adds a section with content; its index.
        pub fn section(
            &mut self,
            name: &str,
            flags: u32,
            addr: u32,
            data: &[u8],
            align: u32,
        ) -> usize {
            self.sections.push(Spec {
                name: name.into(),
                kind: 1,
                flags,
                addr,
                size: data.len() as u32,
                data: data.to_vec(),
                align,
            });
            self.sections.len()
        }

        /// Adds a section without bytes in the file (`SHT_NOBITS`); its
        /// index.
        pub fn nobits(&mut self, name: &str, flags: u32, addr: u32, size: u32) -> usize {
            self.sections.push(Spec {
                name: name.into(),
                kind: SHT_NOBITS,
                flags,
                addr,
                data: Vec::new(),
                size,
                align: 4,
            });
            self.sections.len()
        }

        /// Adds a symbol (global unless `local`); its index.
        pub fn symbol(
            &mut self,
            name: &str,
            value: u32,
            size: u32,
            shndx: u16,
            local: bool,
        ) -> u32 {
            self.symbols.push(Symbol {
                name: name.into(),
                value,
                size,
                info: if local { 0 } else { 0x10 },
                shndx,
            });
            self.symbols.len() as u32
        }

        /// Adds a function symbol (`STT_FUNC`, global unless `local`) of
        /// `size` bytes; its index.
        pub fn function(
            &mut self,
            name: &str,
            value: u32,
            size: u32,
            shndx: u16,
            local: bool,
        ) -> u32 {
            let index = self.symbol(name, value, size, shndx, local);
            let last = self.symbols.last_mut().expect("just pushed");
            last.info |= 2;
            index
        }

        /// Adds a relocation of `kind` against `symbol` at `offset` in
        /// `section`.
        pub fn rela(&mut self, section: usize, offset: u32, symbol: u32, kind: u32) {
            let rela = Rela {
                offset,
                symbol,
                kind,
                addend: 0,
            };
            match self.relas.iter_mut().find(|(s, _)| *s == section) {
                Some((_, list)) => list.push(rela),
                None => self.relas.push((section, vec![rela])),
            }
        }

        pub fn build(&self) -> Vec<u8> {
            let mut names = vec![0u8];
            let mut name = |text: &str| {
                let at = names.len() as u32;
                names.extend_from_slice(text.as_bytes());
                names.push(0);
                at
            };
            let mut strtab = vec![0u8];
            let mut symtab = vec![0u8; SYM_LEN];
            for symbol in &self.symbols {
                let at = strtab.len() as u32;
                strtab.extend_from_slice(symbol.name.as_bytes());
                strtab.push(0);
                symtab.extend_from_slice(&at.to_le_bytes());
                symtab.extend_from_slice(&symbol.value.to_le_bytes());
                symtab.extend_from_slice(&symbol.size.to_le_bytes());
                symtab.push(symbol.info);
                symtab.push(0);
                symtab.extend_from_slice(&symbol.shndx.to_le_bytes());
            }
            let symtab_index = (self.sections.len() + self.relas.len() + 1) as u32;
            let mut all = self.sections.clone();
            for (section, list) in &self.relas {
                let mut data = Vec::new();
                for r in list {
                    data.extend_from_slice(&r.offset.to_le_bytes());
                    data.extend_from_slice(&((r.symbol << 8) | r.kind).to_le_bytes());
                    data.extend_from_slice(&r.addend.to_le_bytes());
                }
                all.push(Spec {
                    name: format!(".rela{}", self.sections[section - 1].name),
                    kind: SHT_RELA,
                    flags: 0x40,
                    addr: 0,
                    size: data.len() as u32,
                    data,
                    align: 4,
                });
            }
            let tables = [
                (".symtab", SHT_SYMTAB, symtab),
                (".strtab", 3, strtab),
                (".shstrtab", 3, Vec::new()),
            ];
            for (table, kind, data) in tables {
                all.push(Spec {
                    name: table.into(),
                    kind,
                    flags: 0,
                    addr: 0,
                    size: data.len() as u32,
                    data,
                    align: 1,
                });
            }
            let name_at: Vec<u32> = all.iter().map(|s| name(&s.name)).collect();
            let last = all.len() - 1;
            all[last].size = names.len() as u32;
            all[last].data = names;

            let mut file = vec![0u8; EHDR_LEN];
            let mut headers = vec![0u8; SHDR_LEN];
            for (i, spec) in all.iter().enumerate() {
                let offset = file.len() as u32;
                file.extend_from_slice(&spec.data);
                let (link, info) = match spec.kind {
                    SHT_RELA => (symtab_index, self.relas[i - self.sections.len()].0 as u32),
                    SHT_SYMTAB => (symtab_index + 1, 1),
                    _ => (0, 0),
                };
                for value in [
                    name_at[i],
                    spec.kind,
                    spec.flags,
                    spec.addr,
                    offset,
                    spec.size,
                    link,
                    info,
                    spec.align,
                    if spec.kind == SHT_SYMTAB { 16 } else { 0 },
                ] {
                    headers.extend_from_slice(&value.to_le_bytes());
                }
            }
            let shoff = file.len() as u32;
            file.extend_from_slice(&headers);
            file[..4].copy_from_slice(b"\x7fELF");
            file[4] = 1;
            file[5] = 1;
            file[6] = 1;
            file[16..18].copy_from_slice(&self.kind.to_le_bytes());
            file[18..20].copy_from_slice(&self.machine.to_le_bytes());
            file[20..24].copy_from_slice(&1u32.to_le_bytes());
            file[32..36].copy_from_slice(&shoff.to_le_bytes());
            file[40..42].copy_from_slice(&(EHDR_LEN as u16).to_le_bytes());
            file[46..48].copy_from_slice(&(SHDR_LEN as u16).to_le_bytes());
            file[48..50].copy_from_slice(&((all.len() + 1) as u16).to_le_bytes());
            file[50..52].copy_from_slice(&(all.len() as u16).to_le_bytes());
            file
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build::Builder;
    use super::*;

    #[test]
    fn a_file_reads_back_as_written() {
        let mut b = Builder::new(EM_RISCV, ET_EXEC);
        let text = b.section(".text", SHF_ALLOC | 4, 0x1000, &[1, 2, 3, 4], 4);
        let bss = b.nobits(".bss", SHF_ALLOC | SHF_WRITE, 0x2000, 64);
        let main = b.symbol("main", 0x1000, 4, text as u16, false);
        b.rela(text, 0x1000, main, 18);
        let file = b.build();

        let elf = Elf::parse(&file).unwrap();
        assert_eq!((elf.kind, elf.machine), (ET_EXEC, EM_RISCV));
        let (index, section) = elf.section(".text").unwrap();
        assert_eq!(index, text);
        assert_eq!(section.addr, 0x1000);
        assert_eq!(elf.data(section).unwrap(), [1, 2, 3, 4]);
        assert!(section.is_alloc() && !section.is_writable());
        let (index, section) = elf.section(".bss").unwrap();
        assert_eq!(index, bss);
        assert!(section.is_writable());
        assert_eq!((section.size, elf.data(section).unwrap().len()), (64, 0));

        let (symtab, symbols) = elf.symbols().unwrap().unwrap();
        assert_eq!(elf.sections[symtab].kind, SHT_SYMTAB);
        assert_eq!(symbols[main as usize].name, "main");
        assert_eq!(symbols[main as usize].shndx, text as u16);
        let (_, rela) = elf.section(".rela.text").unwrap();
        assert_eq!(rela.info as usize, text);
        assert_eq!(
            elf.relas(rela).unwrap(),
            [Rela {
                offset: 0x1000,
                symbol: main,
                kind: 18,
                addend: 0
            }]
        );
    }

    #[test]
    fn what_is_no_elf32_file_is_said() {
        assert_eq!(Elf::parse(b"!<arch>\n").unwrap_err(), "not an ELF file");
        assert_eq!(
            Elf::parse(b"\x7fELF\x01\x01").unwrap_err(),
            "the ELF file is cut short"
        );
        let mut file = Builder::new(EM_XTENSA, ET_REL).build();
        file[4] = 2;
        assert_eq!(
            Elf::parse(&file).unwrap_err(),
            "not a 32-bit little-endian ELF file"
        );
        let mut file = Builder::new(EM_XTENSA, ET_REL).build();
        let at = file.len() - 4;
        file.truncate(at);
        assert_eq!(
            Elf::parse(&file).unwrap_err(),
            "its section headers lie outside the file"
        );
    }
}
