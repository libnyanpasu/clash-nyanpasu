pub(super) fn selected_title(s: impl AsRef<str>) -> String {
    format!("{} ✔", s.as_ref())
}
