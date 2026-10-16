use half::f16;
use memmap2::MmapMut;

pub const VECTOR_DIMENSION: usize = 768;
pub const VECTOR_BYTES: usize = VECTOR_DIMENSION * std::mem::size_of::<f16>();

pub struct Vectors;

impl Vectors {
    pub const DIMENSION: usize = VECTOR_DIMENSION;
    pub const BYTES: usize = VECTOR_BYTES;

    /// Converts a 768-dimensional f32 vector into a 1,536-byte Little-Endian f16 byte buffer.
    pub fn half(vector: &[f32]) -> Vec<u8> {
        assert_eq!(
            vector.len(),
            Self::DIMENSION,
            "Vector length must be 768 dimensions, got {}",
            vector.len()
        );

        let mut bytes = Vec::with_capacity(Self::BYTES);
        for &val in vector {
            let h = f16::from_f32(val);
            bytes.extend_from_slice(&h.to_le_bytes());
        }
        bytes
    }

    /// Converts a 1,536-byte f16 Little-Endian slice into a 768-dimensional f32 vector.
    pub fn to_floats(bytes: &[u8]) -> Vec<f32> {
        assert_eq!(
            bytes.len(),
            Self::BYTES,
            "Bytes length must be exactly 1536 bytes, got {}",
            bytes.len()
        );

        let mut floats = Vec::with_capacity(Self::DIMENSION);
        for chunk in bytes.chunks_exact(2) {
            let h = f16::from_le_bytes([chunk[0], chunk[1]]);
            floats.push(h.to_f32());
        }
        floats
    }

    /// In-place batch conversion from f16 slice to f32 destination buffer.
    pub fn convert_halves_to_floats(source: &[f16], destination: &mut [f32]) {
        assert_eq!(
            source.len(),
            destination.len(),
            "Source and destination slices must have equal length."
        );
        for (src, dest) in source.iter().zip(destination.iter_mut()) {
            *dest = src.to_f32();
        }
    }

    pub fn scores(rows: &[f32], query: &[f32], scores_out: &mut [f32]) {
        assert_eq!(query.len(), Self::DIMENSION, "Query must be 768 dimensions");
        let count = scores_out.len();
        assert_eq!(
            rows.len(),
            count * Self::DIMENSION,
            "Rows slice length must equal count * 768"
        );

        for (row_idx, score) in scores_out.iter_mut().enumerate() {
            let row_start = row_idx * Self::DIMENSION;
            let row = &rows[row_start..row_start + Self::DIMENSION];

            let mut i = 0;
            let mut dot = 0.0f32;

            while i < Self::DIMENSION {
                dot += row[i] * query[i]
                    + row[i + 1] * query[i + 1]
                    + row[i + 2] * query[i + 2]
                    + row[i + 3] * query[i + 3]
                    + row[i + 4] * query[i + 4]
                    + row[i + 5] * query[i + 5]
                    + row[i + 6] * query[i + 6]
                    + row[i + 7] * query[i + 7];
                i += 8;
            }
            *score = dot;
        }
    }
}

pub struct VectorStorage {
    rows: usize,
    mmap: MmapMut,
    block_offset: usize,
}

impl VectorStorage {
    pub const BLOCK_ROWS: usize = 1024;

    pub fn new(rows: usize) -> std::io::Result<Self> {
        if rows == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "VectorStorage row must be greater than zero.",
            ));
        }

        let half_bytes = rows * VECTOR_DIMENSION * std::mem::size_of::<f16>();
        let block_bytes = Self::BLOCK_ROWS * VECTOR_DIMENSION * std::mem::size_of::<f32>();
        let total_bytes = half_bytes + block_bytes;

        let mmap = MmapMut::map_anon(total_bytes)?;

        Ok(VectorStorage {
            rows,
            mmap,
            block_offset: half_bytes,
        })
    }

    #[inline]
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Read only access to all stored f16 vectors.
    pub fn halves(&self) -> &[f16] {
        let total_elements = self.rows * self::VECTOR_DIMENSION;
        unsafe { std::slice::from_raw_parts(self.mmap.as_ptr() as *const f16, total_elements) }
    }

    /// Mutable access to fill stored f16 vectors.
    pub fn haves_mut(&mut self) -> &mut [f16] {
        let total_elements = self.rows * self::VECTOR_DIMENSION;
        unsafe {
            std::slice::from_raw_parts_mut(self.mmap.as_mut_ptr() as *mut f16, total_elements)
        }
    }

    /// Scratch buffer slice (1024 * 768 f32s) for converting a batch of rows during scoring.
    pub fn scratch_block_mut(&mut self) -> &mut [f32] {
        let total_elements = Self::BLOCK_ROWS * VECTOR_DIMENSION;
        unsafe {
            let ptr = self.mmap.as_mut_ptr().add(self.block_offset) as *mut f32;
            std::slice::from_raw_parts_mut(ptr, total_elements)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_vector_half_roundtrip() {
        let original: Vec<f32> = (0..Vectors::DIMENSION)
            .map(|i| (i as f32) / 100.0)
            .collect();
        let bytes = Vectors::half(&original);
        assert_eq!(bytes.len(), Vectors::BYTES);
        let restored = Vectors::to_floats(&bytes);
        assert_eq!(restored.len(), Vectors::DIMENSION);
        // Verify float16 precision limits (~0.001 tolerance)
        for (a, b) in original.iter().zip(restored.iter()) {
            assert!(
                (a - b).abs() < 0.01,
                "Mismatch: orig {} vs restored {}",
                a,
                b
            );
        }
    }
    #[test]
    fn test_vector_storage_allocation() {
        let rows = 2048;
        let mut storage = VectorStorage::new(rows).expect("Failed to allocate VectorStorage");
        assert_eq!(storage.rows(), rows);
        assert_eq!(storage.halves().len(), rows * Vectors::DIMENSION);
        assert_eq!(
            storage.scratch_block_mut().len(),
            VectorStorage::BLOCK_ROWS * Vectors::DIMENSION
        );
    }

    #[test]
    fn test_vector_scores() {
        let query = vec![1.0f32; Vectors::DIMENSION];
        let mut rows = Vec::with_capacity(2 * Vectors::DIMENSION);
        rows.extend(vec![0.5f32; Vectors::DIMENSION]);
        rows.extend(vec![2.0f32; Vectors::DIMENSION]);

        let mut scores = vec![0.0f32; 2];
        Vectors::scores(&rows, &query, &mut scores);

        assert_eq!(scores[0], 768.0 * 0.5); // 384.0
        assert_eq!(scores[1], 768.0 * 2.0); // 1536.0
    }
}
