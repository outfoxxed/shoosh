/// A fixed length buffer that reuses old element memory to insert new elements.
///
/// Only contains functionality required for shoosh.
#[derive(Debug)]
pub struct RingBuffer<T: Clone> {
	buffer: Vec<T>,
	size: usize,
	index: usize,
}

impl<T: Clone> RingBuffer<T> {
	/// Create a new ring buffer
	pub fn new(size: usize) -> Self {
		Self {
			buffer: Vec::with_capacity(size),
			size,
			index: 0,
		}
	}

	/// Returns an iterator over all ring buffer elements.
	pub fn iter(&self) -> impl Iterator<Item = &T> {
		self.buffer[self.index..]
			.iter()
			.chain(&self.buffer[..self.index])
	}

	/// Appends a slice of values into the ring buffer.
	/// Only the last <size of ring> elements are kept.
	pub fn append(&mut self, mut elements: &[T]) {
		// only insert elements that can fit in the buffer
		let count = elements.len().checked_sub(self.size).unwrap_or(0);
		elements = &elements[count..];

		let (tail_elements, head_elements) =
			elements.split_at((self.size - self.index).min(elements.len()));
		if self.buffer.len() < self.size {
			// Extend the array with tail elements if not present.
			// Overflow will be inserted at head below.
			self.buffer.extend_from_slice(tail_elements);
		} else {
			// If the appended element array is smaller than `size - index`
			// it will be inserted at `index` in place and the below head insert
			// will be a no-op.
			self.buffer[self.index..(self.index + tail_elements.len())]
				.clone_from_slice(tail_elements);
		}

		self.buffer[..head_elements.len()].clone_from_slice(head_elements);

		self.index = (self.index + elements.len()) % self.size;
	}
}

#[cfg(test)]
mod test {
	use super::RingBuffer;

	fn collect_buffer<T: Clone>(buffer: &RingBuffer<T>) -> Vec<T> {
		buffer.iter().cloned().collect::<Vec<_>>()
	}

	#[test]
	fn append_elements() {
		let mut buffer = RingBuffer::new(5);

		// partial insert
		buffer.append(&[1, 2]);
		assert_eq!(&[1, 2], collect_buffer(&buffer).as_slice());

		// wrap ring before full
		buffer.append(&[3, 4, 5, 6]);
		assert_eq!(&[2, 3, 4, 5, 6], collect_buffer(&buffer).as_slice());

		// insert elements in the middle
		buffer.append(&[7, 8]);
		assert_eq!(&[4, 5, 6, 7, 8], collect_buffer(&buffer).as_slice());

		// wrap ring while full
		buffer.append(&[9, 10, 11]);
		assert_eq!(&[7, 8, 9, 10, 11], collect_buffer(&buffer).as_slice());

		// insert more elements than ring holds
		buffer.append(&[12, 13, 14, 15, 16, 17, 18, 19, 20]);
		assert_eq!(&[16, 17, 18, 19, 20], collect_buffer(&buffer).as_slice());
	}
}
