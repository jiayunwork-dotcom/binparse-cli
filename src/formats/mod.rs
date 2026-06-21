use crate::dsl::FormatDefinition;
use anyhow::Result;
use std::collections::HashMap;

pub const PNG_YAML: &str = r#"
name: PNG
magic: [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
enums:
  - name: ColorType
    values:
      GRAYSCALE: 0
      RGB: 2
      INDEXED: 3
      GRAYSCALE_ALPHA: 4
      RGBA: 6
structs:
  - name: IHDRChunk
    fields:
      - name: length
        type: u32
        offset: "0"
        endian: big
        format: dec
      - name: chunk_type
        type:
          string:
            length: "4"
            encoding: ascii
        offset: relative
      - name: width
        type: u32
        offset: relative
        endian: big
        format: dec
      - name: height
        type: u32
        offset: relative
        endian: big
        format: dec
      - name: bit_depth
        type: u8
        offset: relative
        format: dec
      - name: color_type
        type:
          enum:
            name: ColorType
            underlying: u8
        offset: relative
      - name: compression_method
        type: u8
        offset: relative
        format: dec
      - name: filter_method
        type: u8
        offset: relative
        format: dec
      - name: interlace_method
        type: u8
        offset: relative
        format: dec
      - name: crc
        type: u32
        offset: relative
        endian: big
        format: hex
        checksum:
          algorithm: crc32
          start: "4"
          end: "21"
root:
  name: PNGFile
  fields:
    - name: signature
      type:
        bytes:
          length: "8"
      offset: "0"
      format: hex
    - name: ihdr
      type:
        struct: IHDRChunk
      offset: relative
"#;

pub const BMP_YAML: &str = r#"
name: BMP
magic: [0x42, 0x4D]
enums:
  - name: Compression
    values:
      BI_RGB: 0
      BI_RLE8: 1
      BI_RLE4: 2
      BI_BITFIELDS: 3
      BI_JPEG: 4
      BI_PNG: 5
structs:
  - name: BMPFileHeader
    fields:
      - name: signature
        type:
          string:
            length: "2"
            encoding: ascii
        offset: "0"
      - name: file_size
        type: u32
        offset: relative
        format: dec
      - name: reserved1
        type: u16
        offset: relative
        format: hex
      - name: reserved2
        type: u16
        offset: relative
        format: hex
      - name: data_offset
        type: u32
        offset: relative
        format: hex
  - name: DIBHeader
    fields:
      - name: header_size
        type: u32
        offset: "0"
        format: dec
      - name: width
        type: i32
        offset: relative
        format: dec
      - name: height
        type: i32
        offset: relative
        format: dec
      - name: planes
        type: u16
        offset: relative
        format: dec
      - name: bits_per_pixel
        type: u16
        offset: relative
        format: dec
      - name: compression
        type:
          enum:
            name: Compression
            underlying: u32
        offset: relative
      - name: image_size
        type: u32
        offset: relative
        format: dec
      - name: x_pixels_per_meter
        type: i32
        offset: relative
        format: dec
      - name: y_pixels_per_meter
        type: i32
        offset: relative
        format: dec
      - name: colors_used
        type: u32
        offset: relative
        format: dec
      - name: colors_important
        type: u32
        offset: relative
        format: dec
root:
  name: BMPFile
  fields:
    - name: file_header
      type:
        struct: BMPFileHeader
      offset: "0"
    - name: dib_header
      type:
        struct: DIBHeader
      offset: "14"
"#;

pub const WAV_YAML: &str = r#"
name: WAV
magic: [0x52, 0x49, 0x46, 0x46]
enums:
  - name: AudioFormat
    values:
      PCM: 1
      ADPCM: 2
      IEEE_FLOAT: 3
      ALAW: 6
      MULAW: 7
structs:
  - name: RIFFChunk
    fields:
      - name: chunk_id
        type:
          string:
            length: "4"
            encoding: ascii
        offset: "0"
      - name: chunk_size
        type: u32
        offset: relative
        format: dec
      - name: format
        type:
          string:
            length: "4"
            encoding: ascii
        offset: relative
  - name: FmtChunk
    fields:
      - name: subchunk_id
        type:
          string:
            length: "4"
            encoding: ascii
        offset: "0"
      - name: subchunk_size
        type: u32
        offset: relative
        format: dec
      - name: audio_format
        type:
          enum:
            name: AudioFormat
            underlying: u16
        offset: relative
      - name: num_channels
        type: u16
        offset: relative
        format: dec
      - name: sample_rate
        type: u32
        offset: relative
        format: dec
      - name: byte_rate
        type: u32
        offset: relative
        format: dec
      - name: block_align
        type: u16
        offset: relative
        format: dec
      - name: bits_per_sample
        type: u16
        offset: relative
        format: dec
  - name: DataChunk
    fields:
      - name: subchunk_id
        type:
          string:
            length: "4"
            encoding: ascii
        offset: "0"
      - name: subchunk_size
        type: u32
        offset: relative
        format: dec
root:
  name: WAVFile
  fields:
    - name: riff
      type:
        struct: RIFFChunk
      offset: "0"
    - name: fmt
      type:
        struct: FmtChunk
      offset: "12"
    - name: data
      type:
        struct: DataChunk
      offset: "36"
"#;

pub const ZIP_YAML: &str = r#"
name: ZIP
magic: [0x50, 0x4B, 0x03, 0x04]
enums:
  - name: CompressionMethod
    values:
      STORE: 0
      DEFLATE: 8
      BZIP2: 12
      LZMA: 14
structs:
  - name: LocalFileHeader
    fields:
      - name: signature
        type: u32
        offset: "0"
        format: hex
      - name: version_needed
        type: u16
        offset: relative
        format: hex
      - name: flags
        type: u16
        offset: relative
        format: hex
      - name: compression_method
        type:
          enum:
            name: CompressionMethod
            underlying: u16
        offset: relative
      - name: last_mod_time
        type: u16
        offset: relative
        format: hex
      - name: last_mod_date
        type: u16
        offset: relative
        format: hex
      - name: crc32
        type: u32
        offset: relative
        format: hex
      - name: compressed_size
        type: u32
        offset: relative
        format: dec
      - name: uncompressed_size
        type: u32
        offset: relative
        format: dec
      - name: filename_length
        type: u16
        offset: relative
        format: dec
      - name: extra_field_length
        type: u16
        offset: relative
        format: dec
      - name: filename
        type:
          string:
            length: filename_length
            encoding: utf8
        offset: relative
      - name: extra_field
        type:
          bytes:
            length: extra_field_length
        offset: relative
        format: hex
root:
  name: ZIPFile
  fields:
    - name: local_file_header
      type:
        struct: LocalFileHeader
      offset: "0"
"#;

pub const ELF_YAML: &str = r#"
name: ELF
magic: [0x7F, 0x45, 0x4C, 0x46]
enums:
  - name: EI_CLASS
    values:
      ELFCLASS32: 1
      ELFCLASS64: 2
  - name: EI_DATA
    values:
      ELFDATA2LSB: 1
      ELFDATA2MSB: 2
  - name: EI_OSABI
    values:
      ELFOSABI_NONE: 0
      ELFOSABI_SYSV: 0
      ELFOSABI_HPUX: 1
      ELFOSABI_NETBSD: 2
      ELFOSABI_LINUX: 3
  - name: E_TYPE
    values:
      ET_NONE: 0
      ET_REL: 1
      ET_EXEC: 2
      ET_DYN: 3
      ET_CORE: 4
  - name: E_MACHINE
    values:
      EM_NONE: 0
      EM_386: 3
      EM_X86_64: 62
      EM_ARM: 40
      EM_AARCH64: 183
structs:
  - name: ELFHeader32
    fields:
      - name: e_ident_magic
        type:
          bytes:
            length: "4"
        offset: "0"
        format: hex
      - name: e_ident_class
        type:
          enum:
            name: EI_CLASS
            underlying: u8
        offset: relative
      - name: e_ident_data
        type:
          enum:
            name: EI_DATA
            underlying: u8
        offset: relative
      - name: e_ident_version
        type: u8
        offset: relative
        format: dec
      - name: e_ident_osabi
        type:
          enum:
            name: EI_OSABI
            underlying: u8
        offset: relative
      - name: e_ident_abiversion
        type: u8
        offset: relative
        format: dec
      - name: e_ident_pad
        type:
          bytes:
            length: "7"
        offset: relative
        format: hex
      - name: e_type
        type:
          enum:
            name: E_TYPE
            underlying: u16
        offset: relative
      - name: e_machine
        type:
          enum:
            name: E_MACHINE
            underlying: u16
        offset: relative
      - name: e_version
        type: u32
        offset: relative
        format: dec
      - name: e_entry
        type: u32
        offset: relative
        format: hex
      - name: e_phoff
        type: u32
        offset: relative
        format: hex
      - name: e_shoff
        type: u32
        offset: relative
        format: hex
      - name: e_flags
        type: u32
        offset: relative
        format: hex
      - name: e_ehsize
        type: u16
        offset: relative
        format: dec
      - name: e_phentsize
        type: u16
        offset: relative
        format: dec
      - name: e_phnum
        type: u16
        offset: relative
        format: dec
      - name: e_shentsize
        type: u16
        offset: relative
        format: dec
      - name: e_shnum
        type: u16
        offset: relative
        format: dec
      - name: e_shstrndx
        type: u16
        offset: relative
        format: dec
  - name: ELFHeader64
    fields:
      - name: e_ident_magic
        type:
          bytes:
            length: "4"
        offset: "0"
        format: hex
      - name: e_ident_class
        type:
          enum:
            name: EI_CLASS
            underlying: u8
        offset: relative
      - name: e_ident_data
        type:
          enum:
            name: EI_DATA
            underlying: u8
        offset: relative
      - name: e_ident_version
        type: u8
        offset: relative
        format: dec
      - name: e_ident_osabi
        type:
          enum:
            name: EI_OSABI
            underlying: u8
        offset: relative
      - name: e_ident_abiversion
        type: u8
        offset: relative
        format: dec
      - name: e_ident_pad
        type:
          bytes:
            length: "7"
        offset: relative
        format: hex
      - name: e_type
        type:
          enum:
            name: E_TYPE
            underlying: u16
        offset: relative
      - name: e_machine
        type:
          enum:
            name: E_MACHINE
            underlying: u16
        offset: relative
      - name: e_version
        type: u32
        offset: relative
        format: dec
      - name: e_entry
        type: u64
        offset: relative
        format: hex
      - name: e_phoff
        type: u64
        offset: relative
        format: hex
      - name: e_shoff
        type: u64
        offset: relative
        format: hex
      - name: e_flags
        type: u32
        offset: relative
        format: hex
      - name: e_ehsize
        type: u16
        offset: relative
        format: dec
      - name: e_phentsize
        type: u16
        offset: relative
        format: dec
      - name: e_phnum
        type: u16
        offset: relative
        format: dec
      - name: e_shentsize
        type: u16
        offset: relative
        format: dec
      - name: e_shnum
        type: u16
        offset: relative
        format: dec
      - name: e_shstrndx
        type: u16
        offset: relative
        format: dec
root:
  name: ELFFile
  fields:
    - name: ident_class
      type: u8
      offset: "4"
    - name: header32
      type:
        struct: ELFHeader32
      offset: "0"
      condition:
        when: ident_class == 1
    - name: header64
      type:
        struct: ELFHeader64
      offset: "0"
      condition:
        when: ident_class == 2
"#;

pub const PE_YAML: &str = r#"
name: PE
magic: [0x4D, 0x5A]
enums:
  - name: MachineType
    values:
      IMAGE_FILE_MACHINE_I386: 0x014C
      IMAGE_FILE_MACHINE_AMD64: 0x8664
      IMAGE_FILE_MACHINE_ARM: 0x01C0
      IMAGE_FILE_MACHINE_ARM64: 0xAA64
  - name: MagicOptional
    values:
      IMAGE_NT_OPTIONAL_HDR32_MAGIC: 0x10B
      IMAGE_NT_OPTIONAL_HDR64_MAGIC: 0x20B
  - name: Subsystem
    values:
      IMAGE_SUBSYSTEM_UNKNOWN: 0
      IMAGE_SUBSYSTEM_NATIVE: 1
      IMAGE_SUBSYSTEM_WINDOWS_GUI: 2
      IMAGE_SUBSYSTEM_WINDOWS_CUI: 3
structs:
  - name: DOSHeader
    fields:
      - name: e_magic
        type:
          string:
            length: "2"
            encoding: ascii
        offset: "0"
      - name: e_cblp
        type: u16
        offset: relative
        format: hex
      - name: e_cp
        type: u16
        offset: relative
        format: hex
      - name: e_crlc
        type: u16
        offset: relative
        format: hex
      - name: e_cparhdr
        type: u16
        offset: relative
        format: hex
      - name: e_minalloc
        type: u16
        offset: relative
        format: hex
      - name: e_maxalloc
        type: u16
        offset: relative
        format: hex
      - name: e_ss
        type: u16
        offset: relative
        format: hex
      - name: e_sp
        type: u16
        offset: relative
        format: hex
      - name: e_csum
        type: u16
        offset: relative
        format: hex
      - name: e_ip
        type: u16
        offset: relative
        format: hex
      - name: e_cs
        type: u16
        offset: relative
        format: hex
      - name: e_lfarlc
        type: u16
        offset: relative
        format: hex
      - name: e_ovno
        type: u16
        offset: relative
        format: hex
      - name: e_res
        type:
          bytes:
            length: "8"
        offset: relative
        format: hex
      - name: e_oemid
        type: u16
        offset: relative
        format: hex
      - name: e_oeminfo
        type: u16
        offset: relative
        format: hex
      - name: e_res2
        type:
          bytes:
            length: "20"
        offset: relative
        format: hex
      - name: e_lfanew
        type: u32
        offset: relative
        format: hex
  - name: COFFHeader
    fields:
      - name: signature
        type:
          string:
            length: "4"
            encoding: ascii
        offset: "0"
      - name: machine
        type:
          enum:
            name: MachineType
            underlying: u16
        offset: relative
      - name: number_of_sections
        type: u16
        offset: relative
        format: dec
      - name: timestamp
        type: u32
        offset: relative
        format: dec
      - name: symbol_table_offset
        type: u32
        offset: relative
        format: hex
      - name: number_of_symbols
        type: u32
        offset: relative
        format: dec
      - name: optional_header_size
        type: u16
        offset: relative
        format: dec
      - name: characteristics
        type: u16
        offset: relative
        format: hex
  - name: OptionalHeader32
    fields:
      - name: magic
        type:
          enum:
            name: MagicOptional
            underlying: u16
        offset: "0"
      - name: major_linker_version
        type: u8
        offset: relative
        format: dec
      - name: minor_linker_version
        type: u8
        offset: relative
        format: dec
      - name: size_of_code
        type: u32
        offset: relative
        format: dec
      - name: size_of_initialized_data
        type: u32
        offset: relative
        format: dec
      - name: size_of_uninitialized_data
        type: u32
        offset: relative
        format: dec
      - name: address_of_entry_point
        type: u32
        offset: relative
        format: hex
      - name: base_of_code
        type: u32
        offset: relative
        format: hex
      - name: base_of_data
        type: u32
        offset: relative
        format: hex
      - name: image_base
        type: u32
        offset: relative
        format: hex
      - name: section_alignment
        type: u32
        offset: relative
        format: dec
      - name: file_alignment
        type: u32
        offset: relative
        format: dec
      - name: major_operating_system_version
        type: u16
        offset: relative
        format: dec
      - name: minor_operating_system_version
        type: u16
        offset: relative
        format: dec
      - name: major_image_version
        type: u16
        offset: relative
        format: dec
      - name: minor_image_version
        type: u16
        offset: relative
        format: dec
      - name: major_subsystem_version
        type: u16
        offset: relative
        format: dec
      - name: minor_subsystem_version
        type: u16
        offset: relative
        format: dec
      - name: win32_version_value
        type: u32
        offset: relative
        format: hex
      - name: size_of_image
        type: u32
        offset: relative
        format: dec
      - name: size_of_headers
        type: u32
        offset: relative
        format: dec
      - name: checksum
        type: u32
        offset: relative
        format: hex
      - name: subsystem
        type:
          enum:
            name: Subsystem
            underlying: u16
        offset: relative
      - name: dll_characteristics
        type: u16
        offset: relative
        format: hex
      - name: size_of_stack_reserve
        type: u32
        offset: relative
        format: dec
      - name: size_of_stack_commit
        type: u32
        offset: relative
        format: dec
      - name: size_of_heap_reserve
        type: u32
        offset: relative
        format: dec
      - name: size_of_heap_commit
        type: u32
        offset: relative
        format: dec
      - name: loader_flags
        type: u32
        offset: relative
        format: hex
      - name: number_of_rva_and_sizes
        type: u32
        offset: relative
        format: dec
  - name: OptionalHeader64
    fields:
      - name: magic
        type:
          enum:
            name: MagicOptional
            underlying: u16
        offset: "0"
      - name: major_linker_version
        type: u8
        offset: relative
        format: dec
      - name: minor_linker_version
        type: u8
        offset: relative
        format: dec
      - name: size_of_code
        type: u32
        offset: relative
        format: dec
      - name: size_of_initialized_data
        type: u32
        offset: relative
        format: dec
      - name: size_of_uninitialized_data
        type: u32
        offset: relative
        format: dec
      - name: address_of_entry_point
        type: u32
        offset: relative
        format: hex
      - name: base_of_code
        type: u32
        offset: relative
        format: hex
      - name: image_base
        type: u64
        offset: relative
        format: hex
      - name: section_alignment
        type: u32
        offset: relative
        format: dec
      - name: file_alignment
        type: u32
        offset: relative
        format: dec
      - name: major_operating_system_version
        type: u16
        offset: relative
        format: dec
      - name: minor_operating_system_version
        type: u16
        offset: relative
        format: dec
      - name: major_image_version
        type: u16
        offset: relative
        format: dec
      - name: minor_image_version
        type: u16
        offset: relative
        format: dec
      - name: major_subsystem_version
        type: u16
        offset: relative
        format: dec
      - name: minor_subsystem_version
        type: u16
        offset: relative
        format: dec
      - name: win32_version_value
        type: u32
        offset: relative
        format: hex
      - name: size_of_image
        type: u32
        offset: relative
        format: dec
      - name: size_of_headers
        type: u32
        offset: relative
        format: dec
      - name: checksum
        type: u32
        offset: relative
        format: hex
      - name: subsystem
        type:
          enum:
            name: Subsystem
            underlying: u16
        offset: relative
      - name: dll_characteristics
        type: u16
        offset: relative
        format: hex
      - name: size_of_stack_reserve
        type: u64
        offset: relative
        format: dec
      - name: size_of_stack_commit
        type: u64
        offset: relative
        format: dec
      - name: size_of_heap_reserve
        type: u64
        offset: relative
        format: dec
      - name: size_of_heap_commit
        type: u64
        offset: relative
        format: dec
      - name: loader_flags
        type: u32
        offset: relative
        format: hex
      - name: number_of_rva_and_sizes
        type: u32
        offset: relative
        format: dec
root:
  name: PEFile
  fields:
    - name: dos_header
      type:
        struct: DOSHeader
      offset: "0"
    - name: coff_header
      type:
        struct: COFFHeader
      offset: "dos_header.e_lfanew"
    - name: optional_magic
      type: u16
      offset: "dos_header.e_lfanew + 24"
      format: hex
    - name: optional_header32
      type:
        struct: OptionalHeader32
      offset: "dos_header.e_lfanew + 24"
      condition:
        when: optional_magic == 0x10B
    - name: optional_header64
      type:
        struct: OptionalHeader64
      offset: "dos_header.e_lfanew + 24"
      condition:
        when: optional_magic == 0x20B
"#;

pub fn get_builtin_formats() -> HashMap<&'static str, &'static str> {
    let mut formats = HashMap::new();
    formats.insert("png", PNG_YAML);
    formats.insert("bmp", BMP_YAML);
    formats.insert("wav", WAV_YAML);
    formats.insert("zip", ZIP_YAML);
    formats.insert("elf", ELF_YAML);
    formats.insert("pe", PE_YAML);
    formats
}

pub fn get_builtin_format(name: &str) -> Option<&'static str> {
    get_builtin_formats().get(name.to_lowercase().as_str()).copied()
}

pub fn parse_builtin_format(name: &str) -> Result<FormatDefinition> {
    let yaml = get_builtin_format(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown builtin format: {}", name))?;
    let def = FormatDefinition::from_yaml(yaml)?;
    Ok(def)
}

pub fn detect_format(data: &[u8]) -> Option<FormatDefinition> {
    let formats = get_builtin_formats();
    
    for (_, yaml) in formats {
        if let Ok(def) = FormatDefinition::from_yaml(yaml) {
            if let Some(magic) = &def.magic {
                if data.len() >= magic.len() && &data[..magic.len()] == magic.as_slice() {
                    return Some(def);
                }
            }
        }
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_png_format() {
        let def = parse_builtin_format("png").unwrap();
        assert_eq!(def.name, "PNG");
        assert_eq!(def.magic, Some(vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]));
    }

    #[test]
    fn test_detect_png_format() {
        let data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D];
        let def = detect_format(&data).unwrap();
        assert_eq!(def.name, "PNG");
    }

    #[test]
    fn test_detect_bmp_format() {
        let data = vec![0x42, 0x4D, 0x00, 0x00, 0x00, 0x00];
        let def = detect_format(&data).unwrap();
        assert_eq!(def.name, "BMP");
    }

    #[test]
    fn test_detect_wav_format() {
        let data = vec![0x52, 0x49, 0x46, 0x46, 0x00, 0x00, 0x00, 0x00, 0x57, 0x41, 0x56, 0x45];
        let def = detect_format(&data).unwrap();
        assert_eq!(def.name, "WAV");
    }

    #[test]
    fn test_detect_zip_format() {
        let data = vec![0x50, 0x4B, 0x03, 0x04, 0x00, 0x00];
        let def = detect_format(&data).unwrap();
        assert_eq!(def.name, "ZIP");
    }

    #[test]
    fn test_detect_elf_format() {
        let data = vec![0x7F, 0x45, 0x4C, 0x46, 0x01, 0x00];
        let def = detect_format(&data).unwrap();
        assert_eq!(def.name, "ELF");
    }

    #[test]
    fn test_detect_pe_format() {
        let data = vec![0x4D, 0x5A, 0x00, 0x00];
        let def = detect_format(&data).unwrap();
        assert_eq!(def.name, "PE");
    }

    #[test]
    fn test_all_builtin_formats_valid() {
        let formats = get_builtin_formats();
        for (name, yaml) in formats {
            let result = FormatDefinition::from_yaml(yaml);
            assert!(result.is_ok(), "Format {} has invalid YAML: {:?}", name, result.err());
        }
    }
}
