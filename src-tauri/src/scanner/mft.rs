use std::collections::{HashMap, HashSet};

const CONTINUATION_BYTES: usize = size_of::<u64>();
const V2_FIXED_BYTES: usize = 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Record {
    pub reference: u64,
    pub parent_reference: u64,
    pub attributes: u32,
    pub name: Vec<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ParseError {
    TruncatedContinuation,
    TruncatedRecord,
    InvalidRecordLength,
    UnsupportedVersion(u16),
    InvalidNameRange,
}

#[cfg(test)]
pub(super) fn parse_batch(bytes: &[u8]) -> Result<(u64, Vec<Record>), ParseError> {
    parse_batch_filtered(bytes, |_, _, _| true)
}

pub(super) fn parse_batch_filtered<P>(
    bytes: &[u8],
    mut include: P,
) -> Result<(u64, Vec<Record>), ParseError>
where
    P: FnMut(u64, u64, u32) -> bool,
{
    let continuation = read_u64(bytes, 0).ok_or(ParseError::TruncatedContinuation)?;
    let mut records = Vec::new();
    let mut offset = CONTINUATION_BYTES;

    while offset < bytes.len() {
        let record_length = read_u32(bytes, offset).ok_or(ParseError::TruncatedRecord)? as usize;
        if record_length < V2_FIXED_BYTES || record_length > bytes.len() - offset {
            return Err(ParseError::InvalidRecordLength);
        }

        let record = &bytes[offset..offset + record_length];
        let major_version = read_u16(record, 4).ok_or(ParseError::TruncatedRecord)?;
        if major_version != 2 {
            return Err(ParseError::UnsupportedVersion(major_version));
        }

        let name_length = read_u16(record, 56).ok_or(ParseError::TruncatedRecord)? as usize;
        let name_offset = read_u16(record, 58).ok_or(ParseError::TruncatedRecord)? as usize;
        let name_end = name_offset
            .checked_add(name_length)
            .filter(|end| name_offset >= V2_FIXED_BYTES && *end <= record.len())
            .ok_or(ParseError::InvalidNameRange)?;
        if !name_length.is_multiple_of(size_of::<u16>()) {
            return Err(ParseError::InvalidNameRange);
        }

        let reference = read_u64(record, 8).ok_or(ParseError::TruncatedRecord)?;
        let parent_reference = read_u64(record, 16).ok_or(ParseError::TruncatedRecord)?;
        let attributes = read_u32(record, 52).ok_or(ParseError::TruncatedRecord)?;
        if include(reference, parent_reference, attributes) {
            let mut name = Vec::with_capacity(name_length / size_of::<u16>());
            for chunk in record[name_offset..name_end].chunks_exact(size_of::<u16>()) {
                name.push(u16::from_le_bytes([chunk[0], chunk[1]]));
            }
            records.push(Record {
                reference,
                parent_reference,
                attributes,
                name,
            });
        }
        offset += record_length;
    }

    Ok((continuation, records))
}

pub(super) fn subtree_order(records: &[Record], root_reference: u64) -> Result<Vec<usize>, String> {
    let mut references = HashSet::with_capacity(records.len());
    let mut children = HashMap::<u64, Vec<usize>>::new();
    for (index, record) in records.iter().enumerate() {
        if !references.insert(record.reference) {
            return Err(format!(
                "the MFT enumeration returned duplicate file reference {}",
                record.reference
            ));
        }
        children
            .entry(record.parent_reference)
            .or_default()
            .push(index);
    }

    for child_indices in children.values_mut() {
        child_indices.sort_unstable_by(|left, right| {
            records[*left]
                .name
                .cmp(&records[*right].name)
                .then_with(|| records[*left].reference.cmp(&records[*right].reference))
        });
    }

    let mut ordered = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = children
        .remove(&root_reference)
        .unwrap_or_default()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    while let Some(index) = stack.pop() {
        let reference = records[index].reference;
        if !visited.insert(reference) {
            return Err(format!(
                "the MFT subtree contains a cycle at file reference {reference}"
            ));
        }
        ordered.push(index);
        if let Some(descendants) = children.remove(&reference) {
            stack.extend(descendants.into_iter().rev());
        }
    }
    Ok(ordered)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let value = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let value = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(value.try_into().ok()?))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let value = bytes.get(offset..offset.checked_add(8)?)?;
    Some(u64::from_le_bytes(value.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded_record(reference: u64, parent: u64, attributes: u32, name: &str) -> Vec<u8> {
        let name = name.encode_utf16().collect::<Vec<_>>();
        let record_length = V2_FIXED_BYTES + name.len() * size_of::<u16>();
        let mut bytes = vec![0_u8; record_length];
        bytes[0..4].copy_from_slice(&(record_length as u32).to_le_bytes());
        bytes[4..6].copy_from_slice(&2_u16.to_le_bytes());
        bytes[8..16].copy_from_slice(&reference.to_le_bytes());
        bytes[16..24].copy_from_slice(&parent.to_le_bytes());
        bytes[52..56].copy_from_slice(&attributes.to_le_bytes());
        bytes[56..58].copy_from_slice(&((name.len() * 2) as u16).to_le_bytes());
        bytes[58..60].copy_from_slice(&(V2_FIXED_BYTES as u16).to_le_bytes());
        for (index, unit) in name.into_iter().enumerate() {
            let start = V2_FIXED_BYTES + index * 2;
            bytes[start..start + 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn parses_a_complete_v2_batch_without_assuming_nul_termination() {
        let mut bytes = 99_u64.to_le_bytes().to_vec();
        bytes.extend(encoded_record(10, 5, 0x20, "notes.txt"));
        bytes.extend(encoded_record(11, 5, 0x10, "photos"));

        let (continuation, records) = parse_batch(&bytes).expect("parse batch");
        assert_eq!(continuation, 99);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].reference, 10);
        assert_eq!(records[0].parent_reference, 5);
        assert_eq!(records[0].attributes, 0x20);
        assert_eq!(
            records[0].name,
            "notes.txt".encode_utf16().collect::<Vec<_>>()
        );
    }

    #[test]
    fn rejects_truncated_or_incompatible_records() {
        assert_eq!(parse_batch(&[0; 7]), Err(ParseError::TruncatedContinuation));

        let mut invalid_length = 0_u64.to_le_bytes().to_vec();
        invalid_length.extend_from_slice(&20_u32.to_le_bytes());
        invalid_length.extend_from_slice(&[0; 16]);
        assert_eq!(
            parse_batch(&invalid_length),
            Err(ParseError::InvalidRecordLength)
        );

        let mut incompatible = 0_u64.to_le_bytes().to_vec();
        let mut record = encoded_record(1, 0, 0, "file");
        record[4..6].copy_from_slice(&3_u16.to_le_bytes());
        incompatible.extend(record);
        assert_eq!(
            parse_batch(&incompatible),
            Err(ParseError::UnsupportedVersion(3))
        );
    }

    #[test]
    fn filters_records_before_allocating_names() {
        let mut bytes = 2_u64.to_le_bytes().to_vec();
        bytes.extend(encoded_record(10, 5, 0x20, "file.bin"));
        bytes.extend(encoded_record(11, 5, 0x10, "folder"));

        let (_, records) = parse_batch_filtered(&bytes, |_, _, attributes| attributes == 0x10)
            .expect("parse filtered batch");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].reference, 11);
    }

    #[test]
    fn orders_only_the_selected_subtree_with_parents_before_children() {
        let records = vec![
            Record {
                reference: 4,
                parent_reference: 2,
                attributes: 0,
                name: "z.txt".encode_utf16().collect(),
            },
            Record {
                reference: 2,
                parent_reference: 1,
                attributes: 0x10,
                name: "folder".encode_utf16().collect(),
            },
            Record {
                reference: 3,
                parent_reference: 99,
                attributes: 0,
                name: "sibling.txt".encode_utf16().collect(),
            },
            Record {
                reference: 5,
                parent_reference: 1,
                attributes: 0,
                name: "a.txt".encode_utf16().collect(),
            },
        ];

        let ordered = subtree_order(&records, 1).expect("resolve subtree");
        assert_eq!(ordered, vec![3, 1, 0]);
    }

    #[test]
    fn rejects_duplicate_file_references() {
        let record = Record {
            reference: 2,
            parent_reference: 1,
            attributes: 0,
            name: vec![],
        };
        assert!(subtree_order(&[record.clone(), record], 1).is_err());
    }
}
