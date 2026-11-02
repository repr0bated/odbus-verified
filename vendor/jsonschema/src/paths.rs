//! Facilities for working with paths within schemas or validated instances.
use std::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
    sync::{Arc, OnceLock},
};

use crate::keywords::Keyword;

/// A location segment.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LocationSegment<'a> {
    /// Property name within a JSON object.
    Property(&'a str),
    /// JSON Schema keyword.
    Index(usize),
}

impl fmt::Display for LocationSegment<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocationSegment::Property(property) => f.write_str(property),
            LocationSegment::Index(idx) => f.write_str(itoa::Buffer::new().format(*idx)),
        }
    }
}

/// A lazily constructed location within a JSON instance.
///
/// [`LazyLocation`] builds a path incrementally during JSON Schema validation without allocating
/// memory until required by storing each segment on the stack.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LazyLocation<'a, 'b> {
    pub(crate) segment: LocationSegment<'a>,
    pub(crate) parent: Option<&'b LazyLocation<'b, 'a>>,
}

impl Default for LazyLocation<'_, '_> {
    fn default() -> Self {
        LazyLocation::new()
    }
}

impl<'a> LazyLocation<'a, '_> {
    /// Create a root node of a JSON pointer.
    pub const fn new() -> Self {
        LazyLocation {
            // The value does not matter, it will never be used
            segment: LocationSegment::Index(0),
            parent: None,
        }
    }

    /// Push a new segment to the JSON pointer.
    #[inline]
    pub fn push(&'a self, segment: impl Into<LocationSegment<'a>>) -> Self {
        LazyLocation {
            segment: segment.into(),
            parent: Some(self),
        }
    }
}

impl<'a> From<&'a LazyLocation<'_, '_>> for Location {
    fn from(value: &'a LazyLocation<'_, '_>) -> Self {
        let mut capacity = 0;
        let mut string_capacity = 0;
        let mut head = value;

        while let Some(next) = head.parent {
            capacity += 1;
            string_capacity += match head.segment {
                LocationSegment::Property(property) => property.len() + 1,
                LocationSegment::Index(idx) => idx.checked_ilog10().unwrap_or(0) as usize + 2,
            };
            head = next;
        }

        let mut buffer = String::with_capacity(string_capacity);

        let mut segments = Vec::with_capacity(capacity);
        head = value;

        if head.parent.is_some() {
            segments.push(head.segment);
        }

        while let Some(next) = head.parent {
            head = next;
            if head.parent.is_some() {
                segments.push(head.segment);
            }
        }

        for segment in segments.iter().rev() {
            buffer.push('/');
            match segment {
                LocationSegment::Property(property) => {
                    write_escaped_str(&mut buffer, property);
                }
                LocationSegment::Index(idx) => {
                    let mut itoa_buffer = itoa::Buffer::new();
                    buffer.push_str(itoa_buffer.format(*idx));
                }
            }
        }

        Location::from_pointer(buffer)
    }
}

impl<'a> From<&'a Keyword> for LocationSegment<'a> {
    fn from(value: &'a Keyword) -> Self {
        match value {
            Keyword::Builtin(k) => LocationSegment::Property(k.as_str()),
            Keyword::Custom(s) => LocationSegment::Property(s),
        }
    }
}

impl<'a> From<&'a str> for LocationSegment<'a> {
    #[inline]
    fn from(value: &'a str) -> LocationSegment<'a> {
        LocationSegment::Property(value)
    }
}

impl<'a> From<&'a String> for LocationSegment<'a> {
    #[inline]
    fn from(value: &'a String) -> LocationSegment<'a> {
        LocationSegment::Property(value)
    }
}

impl From<usize> for LocationSegment<'_> {
    #[inline]
    fn from(value: usize) -> Self {
        LocationSegment::Index(value)
    }
}

/// A cheap to clone JSON pointer that represents location with a JSON value.
#[derive(Clone)]
pub struct Location(Arc<LocationData>);

struct LocationData {
    pointer: String,
    // Iteration must return borrowed, decoded segments without changing the
    // public Copy LocationSegment representation or leaking allocations.
    decoded: OnceLock<Vec<String>>,
}

impl fmt::Debug for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Location").field(&self.0.pointer).finish()
    }
}

impl PartialEq for Location {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Location {}

impl Hash for Location {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialOrd for Location {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Location {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl serde::Serialize for Location {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl Location {
    /// Create a new, empty `Location`.
    pub fn new() -> Self {
        Self::from_pointer(String::new())
    }
    fn from_pointer(pointer: String) -> Self {
        Self(Arc::new(LocationData {
            pointer,
            decoded: OnceLock::new(),
        }))
    }
    pub fn join<'a>(&self, segment: impl Into<LocationSegment<'a>>) -> Self {
        let parent = self.as_str();
        match segment.into() {
            LocationSegment::Property(property) => {
                let mut buffer = String::with_capacity(parent.len() + property.len() + 1);
                buffer.push_str(parent);
                buffer.push('/');
                write_escaped_str(&mut buffer, property);
                Self::from_pointer(buffer)
            }
            LocationSegment::Index(idx) => {
                let mut buffer = itoa::Buffer::new();
                let segment = buffer.format(idx);
                Self::from_pointer(format!("{parent}/{segment}"))
            }
        }
    }
    /// Get a string slice representing the location.
    pub fn as_str(&self) -> &str {
        &self.0.pointer
    }
    /// Get a byte slice representing the location.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.pointer.as_bytes()
    }
}

fn write_escaped_str(buffer: &mut String, value: &str) {
    match value.find(['~', '/']) {
        Some(mut escape_idx) => {
            let mut remaining = value;

            // Loop through the string to replace `~` and `/`
            loop {
                let (before, after) = remaining.split_at(escape_idx);
                // Copy everything before the escape char
                buffer.push_str(before);

                // Append the appropriate escape sequence
                match after.as_bytes()[0] {
                    b'~' => buffer.push_str("~0"),
                    b'/' => buffer.push_str("~1"),
                    _ => unreachable!(),
                }

                // Move past the escaped character
                remaining = &after[1..];

                // Find the next `~` or `/` to continue escaping
                if let Some(next_escape_idx) = remaining.find(['~', '/']) {
                    escape_idx = next_escape_idx;
                } else {
                    // Append any remaining part of the string
                    buffer.push_str(remaining);
                    break;
                }
            }
        }
        None => {
            // If no escape characters are found, append the segment as is
            buffer.push_str(value);
        }
    };
}

impl Default for Location {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'a> IntoIterator for &'a Location {
    type Item = LocationSegment<'a>;
    type IntoIter = std::vec::IntoIter<LocationSegment<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0
            .decoded
            .get_or_init(|| {
                // Only the first empty component is the pointer's leading slash.
                // Decode ~1 before ~0 so ~01 remains a literal "~1".
                self.as_str()
                    .split('/')
                    .skip(1)
                    .map(|part| part.replace("~1", "/").replace("~0", "~"))
                    .collect()
            })
            .iter()
            .map(|p| {
                // Numeric-looking object keys such as "00" and "+1" must not
                // lose their spelling when the segments are collected again.
                let canonical_integer = p == "0"
                    || (p.as_bytes().first().is_some_and(|b| (b'1'..=b'9').contains(b))
                        && p.bytes().all(|b| b.is_ascii_digit()));
                let index = if canonical_integer {
                    p.parse::<usize>().ok()
                } else {
                    None
                };
                index.map_or(LocationSegment::Property(p), LocationSegment::Index)
            })
            .collect::<Vec<_>>()
            .into_iter()
    }
}

impl<'a> FromIterator<LocationSegment<'a>> for Location {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = LocationSegment<'a>>,
    {
        fn inner<'a, 'b, 'c, I>(path_iter: &mut I, location: &'b LazyLocation<'b, 'a>) -> Location
        where
            I: Iterator<Item = LocationSegment<'c>>,
        {
            let Some(path) = path_iter.next() else {
                return location.into();
            };
            let location = location.push(path);
            inner(path_iter, &location)
        }

        let loc = LazyLocation::default();
        inner(&mut iter.into_iter(), &loc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test]
    fn test_location_default() {
        let loc = Location::default();
        assert_eq!(loc.as_str(), "");
    }

    #[test]
    fn test_location_new() {
        let loc = Location::new();
        assert_eq!(loc.as_str(), "");
    }

    #[test]
    fn test_location_join_property() {
        let loc = Location::new();
        let loc = loc.join("property");
        assert_eq!(loc.as_str(), "/property");
    }

    #[test]
    fn test_location_join_index() {
        let loc = Location::new();
        let loc = loc.join(0);
        assert_eq!(loc.as_str(), "/0");
    }

    #[test]
    fn test_location_join_multiple() {
        let loc = Location::new();
        let loc = loc.join("property").join(0);
        assert_eq!(loc.as_str(), "/property/0");
    }

    #[test]
    fn test_as_bytes() {
        let loc = Location::new().join("test");
        assert_eq!(loc.as_bytes(), b"/test");
    }

    #[test]
    fn test_display_trait() {
        let loc = Location::new().join("property");
        assert_eq!(format!("{}", loc), "/property");
    }

    #[test_case("tilde~character", "/tilde~0character"; "escapes tilde")]
    #[test_case("slash/character", "/slash~1character"; "escapes slash")]
    #[test_case("combo~and/slash", "/combo~0and~1slash"; "escapes tilde and slash combined")]
    #[test_case("multiple~/escapes~", "/multiple~0~1escapes~0"; "multiple escapes")]
    #[test_case("first/segment", "/first~1segment"; "escapes slash in nested segment")]
    fn test_location_escaping(segment: &str, expected: &str) {
        let loc = Location::new().join(segment);
        assert_eq!(loc.as_str(), expected);
    }

    #[test_case("/a/b/c", vec![LocationSegment::Property("a"), LocationSegment::Property("b"), LocationSegment::Property("c")]; "location with properties")]
    #[test_case("/1/2/3", vec![LocationSegment::Index(1), LocationSegment::Index(2), LocationSegment::Index(3)]; "location with indices")]
    #[test_case("/a/1/b/2", vec![
        LocationSegment::Property("a"),
        LocationSegment::Index(1),
        LocationSegment::Property("b"),
        LocationSegment::Index(2)
    ]; "mixed properties and indices")]
    fn test_into_iter(location: &str, expected_segments: Vec<LocationSegment>) {
        let loc = Location::from_pointer(location.to_string());
        assert_eq!(loc.into_iter().collect::<Vec<_>>(), expected_segments);
    }

    #[test_case(vec![LocationSegment::Property("a"), LocationSegment::Property("b")], "/a/b"; "properties only")]
    #[test_case(vec![LocationSegment::Index(1), LocationSegment::Index(2)], "/1/2"; "indices only")]
    #[test_case(vec![LocationSegment::Property("a"), LocationSegment::Index(1)], "/a/1"; "mixed segments")]
    fn test_from_iter(segments: Vec<LocationSegment>, expected: &str) {
        assert_eq!(Location::from_iter(segments).as_str(), expected);
    }

    #[test]
    fn escaped_and_empty_segments_round_trip_without_changing_identity() {
        let location = Location::new().join("").join("a/b").join("~1").join("00").join("+1").join("");
        let untouched = Location::new().join("").join("a/b").join("~1").join("00").join("+1").join("");
        let segments: Vec<_> = location.into_iter().collect();
        assert_eq!(segments, vec![
            LocationSegment::Property(""), LocationSegment::Property("a/b"),
            LocationSegment::Property("~1"), LocationSegment::Property("00"),
            LocationSegment::Property("+1"), LocationSegment::Property(""),
        ]);
        assert_eq!(Location::from_iter(segments), untouched);
        assert_eq!(location, untouched);
        assert_eq!(location.cmp(&untouched), Ordering::Equal);
        let hash = |value: &Location| {
            let mut state = std::collections::hash_map::DefaultHasher::new();
            value.hash(&mut state);
            state.finish()
        };
        assert_eq!(hash(&location), hash(&untouched));
        assert!(Location::new().into_iter().next().is_none());
        assert_eq!(Location::new().join("").into_iter().count(), 1);
        let clone = location.clone();
        assert_eq!(clone.into_iter().collect::<Location>(), location);
    }
}
