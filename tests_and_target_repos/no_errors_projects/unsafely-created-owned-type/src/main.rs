// Example Usage
// =============

fn main() {
    let string = String::from(" olá. ");
    let owned_sheet = StyleSheetBuf::new(string);
    print_parsed(&owned_sheet);
}

fn print_parsed(sheet: &StyleSheet) {
    println!("parsed: {:?}", sheet.parsed);
}

#[allow(dead_code)]
fn parse_file(path: &str) -> StyleSheetBuf {
    let source = std::fs::read_to_string(path).unwrap();
    StyleSheetBuf::new(source)
}

// Implementation
// ==============

struct StyleSheetBuf {
    source_ptr: *mut String,

    // Why use an option here? See:
    // https://doc.rust-lang.org/nomicon/destructors.html
    sheet: Option<StyleSheet<'static>>,
}

impl StyleSheetBuf {
    /// Creates a new `StyleSheetBuf`.
    pub fn new(source: String) -> StyleSheetBuf {
        // First move the string to the heap so one may then "leak" it and get a static reference.
        let boxed_source = Box::new(source);

        // Returns a raw pointer to the underlying string. This also prevents the `Box`'s
        // destructor to be executed after this method ends. As such our string type will live until
        // one explicitly drops it.
        let source_ptr = Box::into_raw(boxed_source);

        // The pointer is then used to get a `&'static str`, which will then be used to create the
        // underlying `StyleSheet`.
        //
        // SAFETY: The following deref is safe since the pointer was just obtained by using the
        // `Box::into_raw` method, which must return a "safe" pointer that fulfills all the deref
        // requirements.
        let str_ref: &'static str = unsafe { (*source_ptr).as_str() };

        let sheet = StyleSheet::parse(str_ref);
        StyleSheetBuf {
            source_ptr,
            sheet: Some(sheet),
        }
    }

    /// Returns a reference to the underlying `StyleSheet<'static>`.
    pub fn sheet(&self) -> &StyleSheet<'static> {
        // Since here there is no way `self.sheet` to be `None`, if performance is critical, one
        // might use the unsafe `unwrap_unchecked` method instead.
        self.sheet.as_ref().unwrap()
    }
}

impl Drop for StyleSheetBuf {
    fn drop(&mut self) {
        // Ensures the underlying `StyleSheet` is dropped before the string is freed. This is
        // needed to avoid a use after free in the case of `StyleSheet` defining its own destructor
        // which could then use the string.
        drop(self.sheet.take());
        // The `self.sheet` field is now `None`, which means that `StyleSheet`s destructor won't be
        // executed again. It is now safe to free the string.

        // SAFETY: This is safe. A double free can't happen here since Rust guarantees that `drop`
        // is executed only once. The pointer is also valid since it wasn't modified since its
        // creation by `Box::into_raw`.
        unsafe {
            drop(Box::from_raw(self.source_ptr));
        }
    }
}

impl std::ops::Deref for StyleSheetBuf {
    type Target = StyleSheet<'static>;

    fn deref(&self) -> &Self::Target {
        self.sheet()
    }
}

struct StyleSheet<'s> {
    parsed: &'s str,
}

impl<'s> StyleSheet<'s> {
    pub fn parse(source: &'s str) -> Self {
        StyleSheet {
            parsed: source.trim(),
        }
    }
}

impl Drop for StyleSheet<'_> {
    fn drop(&mut self) {
        // This code may safely use `self.parsed`.
    }
}
