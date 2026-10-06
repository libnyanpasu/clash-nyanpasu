use std::borrow::Cow;

#[allow(unused)]
pub fn escape(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();

    let mut owned = None;

    for pos in 0..bytes.len() {
        let special = match bytes[pos] {
            b' ' => Some(b' '),
            _ => None,
        };
        if let Some(s) = special {
            if owned.is_none() {
                owned = Some(bytes[0..pos].to_owned());
            }
            owned.as_mut().unwrap().push(b'\\');
            owned.as_mut().unwrap().push(b'\\');
            owned.as_mut().unwrap().push(s);
        } else if let Some(owned) = owned.as_mut() {
            owned.push(bytes[pos]);
        }
    }

    if let Some(owned) = owned {
        Cow::Owned(String::from_utf8(owned).unwrap())
    } else {
        Cow::Borrowed(std::str::from_utf8(bytes).unwrap())
    }
}
