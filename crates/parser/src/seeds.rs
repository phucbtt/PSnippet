use zs_core::{FieldDefinition, FieldType, MentionFallback, Snippet};

use crate::legacy::parse_legacy;

/// The seven seed templates, verbatim from spec §4.2.
pub const DEFAULT_SEEDS_TEXT: &str = include_str!("../../../examples/default-snippets.ahk.txt");

const META: &[(&str, &str, &str)] = &[
    ("ztn", "Tiếp nhận hỗ trợ", "Tiếp nhận"),
    ("ztnr", "Đã tiếp nhận qua kênh riêng", "Đã tiếp nhận riêng"),
    ("zxl", "Đang kiểm tra xử lý", "Đang xử lý"),
    ("ztd", "Cập nhật tiến độ", "Cập nhật tiến độ"),
    ("zok", "Đã xử lý xong", "Hoàn tất"),
    ("zgap", "Ghi nhận phản ánh", "Phản ánh"),
    ("zbackup", "CS backup tiếp nhận", "Bàn giao"),
];

pub const DEFAULT_CATEGORIES: &[&str] = &[
    "Tiếp nhận",
    "Đã tiếp nhận riêng",
    "Đang xử lý",
    "Cập nhật tiến độ",
    "Hoàn tất",
    "Phản ánh",
    "Bàn giao",
];

/// Seed snippets: bodies untouched; `[Nội dung]` is made multiline via field metadata (§4A.2 #4).
pub fn seed_snippets() -> Vec<Snippet> {
    let parsed = parse_legacy(DEFAULT_SEEDS_TEXT).expect("embedded seeds are valid");
    parsed
        .into_iter()
        .map(|p| {
            let mut s = p.into_snippet();
            if let Some((_, title, cat)) = META.iter().find(|(c, _, _)| *c == s.code) {
                s.title = (*title).to_string();
                s.category = Some((*cat).to_string());
            }
            if let Ok(t) = zs_core::Template::parse(&s.body)
                && t.fields.iter().any(|f| f.label == "Nội dung")
            {
                s.fields.push(FieldDefinition {
                    key: "Nội dung".into(),
                    field_type: FieldType::Multiline,
                    required: true,
                    mention_fallback: MentionFallback::RequireReal,
                    default_value: None,
                });
            }
            s
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_seeds_exact_bodies() {
        let seeds = seed_snippets();
        let codes: Vec<_> = seeds.iter().map(|s| s.code.as_str()).collect();
        assert_eq!(
            codes,
            ["ztn", "ztnr", "zxl", "ztd", "zok", "zgap", "zbackup"]
        );
        assert_eq!(
            seeds[0].body,
            "Hiện tại em đã nhận được thông tin hỗ trợ từ chị [Tên].\nBên em đã ghi nhận nội dung về [Nội dung] và đang kiểm tra để có hướng xử lý phù hợp ạ.\nEm sẽ cập nhật lại chị ngay khi có thông tin mới nhé!"
        );
        // ztd has 3 fields in order of appearance
        let t = seeds[3].template().unwrap();
        let labels: Vec<_> = t.fields.iter().map(|f| f.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Nội dung", "Tiến độ đã thực hiện", "Công việc tiếp theo"]
        );
        assert_eq!(t.fields[0].field_type, FieldType::Multiline);
        for s in &seeds {
            assert!(s.category.is_some());
            assert!(s.template().is_ok());
        }
    }

    #[test]
    fn seeds_survive_legacy_round_trip() {
        let seeds = seed_snippets();
        let text = crate::legacy::export_legacy(&seeds);
        let back = parse_legacy(&text).unwrap();
        for (a, b) in seeds.iter().zip(back) {
            assert_eq!(a.body, b.body);
            assert_eq!(a.code, b.code);
        }
    }
}
