//! Embeddings for vector recall. The `Embedder` trait is the seam: the default is a
//! dependency-free pure-Rust **feature-hashing** embedder (fuzzy/sub-word matching the
//! FTS index misses); a neural embedder (local ONNX via fastembed, or an Ollama HTTP
//! endpoint) drops in behind this trait later without touching the recall pipeline.
//! See Lectern-Brain/03-Architecture/Memory Engine.md + 09-Deep-Dives/Local Memory & Learning Stack (deep).md.

pub trait Embedder {
    fn dim(&self) -> usize;
    /// Returns an L2-normalized embedding (so dot product == cosine similarity).
    fn embed(&self, text: &str) -> Vec<f32>;
    fn id(&self) -> &str;
}

/// Pure-Rust feature-hashing embedder: hashes word tokens + character trigrams into a
/// fixed-dim signed vector, then L2-normalizes. No model, no downloads, deterministic.
pub struct HashEmbedder {
    pub dim: usize,
}

impl HashEmbedder {
    pub fn new() -> Self {
        Self { dim: 256 }
    }
}

impl Default for HashEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn add_feature(v: &mut [f32], feat: &[u8]) {
    let h = fnv1a(feat);
    let idx = (h as usize) % v.len();
    let sign = if (h >> 63) & 1 == 1 { 1.0 } else { -1.0 };
    v[idx] += sign;
}

impl Embedder for HashEmbedder {
    fn dim(&self) -> usize {
        self.dim
    }
    fn id(&self) -> &str {
        "hash-256"
    }
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dim];
        let lower = text.to_lowercase();
        for tok in lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() >= 2)
        {
            add_feature(&mut v, tok.as_bytes());
            let chars: Vec<char> = tok.chars().collect();
            if chars.len() >= 3 {
                for w in chars.windows(3) {
                    let s: String = w.iter().collect();
                    add_feature(&mut v, s.as_bytes());
                }
            }
        }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }
        v
    }
}

/// Cosine similarity for L2-normalized vectors (== dot product).
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Serialize/deserialize f32 vectors as little-endian bytes for the BLOB column.
pub fn to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

pub fn from_bytes(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_is_deterministic_and_normalized() {
        let e = HashEmbedder::new();
        let a = e.embed("reset the login password");
        let b = e.embed("reset the login password");
        assert_eq!(a.len(), e.dim());
        assert_eq!(a, b); // deterministic — no model, no randomness
        let norm = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "L2-normalized (got norm {norm})");
        // Empty / no usable tokens → the zero vector (norm 0, left un-normalized).
        assert!(e.embed("").iter().all(|&x| x == 0.0));
    }

    #[test]
    fn cosine_ranks_related_and_guards_length() {
        let e = HashEmbedder::new();
        let a = e.embed("login authentication session");
        let related = e.embed("authenticate the login session token");
        let unrelated = e.embed("bake chocolate chip cookies");
        // A related pair scores higher than an unrelated one.
        assert!(cosine(&a, &related) > cosine(&a, &unrelated));
        // Self-similarity of a normalized vector is ~1.
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-4);
        // Mismatched lengths are a guarded 0.0, not a panic.
        assert_eq!(cosine(&[1.0, 0.0], &[1.0]), 0.0);
    }

    #[test]
    fn bytes_roundtrip() {
        let v = vec![0.0f32, -1.5, 3.25, 42.0, -0.001];
        assert_eq!(from_bytes(&to_bytes(&v)), v);
        // A trailing partial chunk is ignored (chunks_exact).
        let mut bytes = to_bytes(&[1.0f32]);
        bytes.push(0xAB);
        assert_eq!(from_bytes(&bytes), vec![1.0f32]);
    }
}
