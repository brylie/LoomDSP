//! Coefficients kept until what they came from changes.

/// Keeps a value until its key changes. A tangent or a power per sample is
/// costly when nothing moves, and a wrapper knows when something did. A NaN
/// never equals itself, which only costs a new compute.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cached<K, V> {
    key: K,
    value: V,
}

impl<K: PartialEq + Copy, V: Copy> Cached<K, V> {
    /// Starts from `key`.
    pub(crate) fn new(key: K, compute: impl Fn(K) -> V) -> Self {
        Self {
            key,
            value: compute(key),
        }
    }

    /// The value for `key`, computing it only if the key changed.
    #[inline]
    pub(crate) fn get(&mut self, key: K, compute: impl Fn(K) -> V) -> V {
        if key != self.key {
            self.key = key;
            self.value = compute(key);
        }
        self.value
    }
}

#[cfg(test)]
mod tests;
