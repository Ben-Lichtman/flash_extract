use std::convert::TryInto;

pub trait Block<'a> {
	type I: Iterator<Item = (Page<'a>, OOB<'a>)>;

	fn from_slice(
		slice: &'a [u8],
		page_size: usize,
		oob_size: usize,
		pages_per_block: usize,
	) -> Vec<Self>
	where
		Self: Sized;

	fn iter(&'a self) -> Self::I;

	fn empty_block(&'a self) -> bool;

	fn bad_block(&'a self) -> bool;

	// fn parity(&'a self) -> Vec<Vec<bool>>;
}

#[derive(Clone, Copy)]
pub struct Page<'a> {
	pub data: &'a [u8],
}

impl Page<'_> {
	pub fn calc_ecc(self) -> u64 {
		let max_pow = (63 - (self.data.len() as u64).leading_zeros()) as usize;

		let mut acc = 0;
		let mut big_acc = [0u64; 64 - 13];

		for (index, chunk) in self.data.chunks_exact(8).enumerate() {
			let chunk = u64::from_be_bytes(chunk.try_into().unwrap());
			acc ^= chunk;

			for pow in 0..max_pow {
				match index & (1 << pow) != 0 {
					false => big_acc[2 * pow] ^= chunk,
					true => big_acc[2 * pow + 1] ^= chunk,
				}
			}
		}

		let parity = acc.count_ones() % 2;

		let p1 = (acc & 0xaaaa_aaaa_aaaa_aaaa).count_ones() % 2;
		let p1_ = (acc & 0x5555_5555_5555_5555).count_ones() % 2;

		let p2 = (acc & 0xcccc_cccc_cccc_cccc).count_ones() % 2;
		let p2_ = (acc & 0x3333_3333_3333_3333).count_ones() % 2;

		let p4 = (acc & 0xf0f0_f0f0_f0f0_f0f0).count_ones() % 2;
		let p4_ = (acc & 0x0f0f_0f0f_0f0f_0f0f).count_ones() % 2;

		let p8 = (acc & 0xff00_ff00_ff00_ff00).count_ones() % 2;
		let p8_ = (acc & 0x00ff_00ff_00ff_00ff).count_ones() % 2;

		let p16 = (acc & 0xffff_0000_ffff_0000).count_ones() % 2;
		let p16_ = (acc & 0x0000_ffff_0000_ffff).count_ones() % 2;

		let p32 = (acc & 0xffff_ffff_0000_0000).count_ones() % 2;
		let p32_ = (acc & 0x0000_0000_ffff_ffff).count_ones() % 2;

		let mut parity = ((parity << 0)
			| (p1 << 1) | (p1_ << 2)
			| (p2 << 3) | (p2_ << 4)
			| (p4 << 5) | (p4_ << 6)
			| (p8 << 7) | (p8_ << 8)
			| (p16 << 9) | p16_ << 10
			| (p32 << 11)
			| (p32_ << 12)) as u64;

		// Parity for 64 128 256 512 1024 2048 calculated here

		for (index, acc) in big_acc.iter().copied().enumerate() {
			if acc.count_ones() % 2 != 0 {
				parity |= 1 << (13 + index)
			}
		}

		parity
	}
}

#[derive(Clone, Copy)]
pub struct OOB<'a> {
	pub data: &'a [u8],
}

pub struct InterlacedBlock<'a> {
	data: Vec<(Page<'a>, OOB<'a>)>,
}

impl<'a> Block<'a> for InterlacedBlock<'a> {
	type I = std::iter::Copied<std::slice::Iter<'a, (Page<'a>, OOB<'a>)>>;

	fn from_slice(
		slice: &'a [u8],
		page_size: usize,
		oob_size: usize,
		pages_per_block: usize,
	) -> Vec<Self> {
		let page_raw_size = page_size + oob_size;
		let block_raw_size = page_raw_size * pages_per_block;

		assert!(slice.len() % block_raw_size == 0);

		slice
			.chunks_exact(block_raw_size)
			.map(|c| {
				let data = c
					.chunks_exact(page_raw_size)
					.map(|p| {
						let page = Page {
							data: &p[..page_size],
						};
						let oob = OOB {
							data: &p[page_size..],
						};
						(page, oob)
					})
					.collect();
				Self { data }
			})
			.collect()
	}

	fn iter(&'a self) -> Self::I { self.data.iter().copied() }

	fn empty_block(&'a self) -> bool {
		for (page, oob) in self.iter() {
			if page.data.iter().copied().any(|x| x != 0xff && x != 0xfe) {
				return false;
			}
			if oob.data.iter().cloned().any(|x| x != 0xff && x != 0xfe) {
				return false;
			}
		}
		true
	}

	fn bad_block(&'a self) -> bool {
		let num_blocks_to_check = 2;
		let byte_offset = 6;

		let mut bad = true;
		for (_, oob) in self.iter().take(num_blocks_to_check) {
			if oob.data[byte_offset] == 0xff {
				bad = false;
			}
		}
		bad
	}
}

pub struct AppendedBlock<'a> {
	data: Vec<(Page<'a>, OOB<'a>)>,
}

impl<'a> Block<'a> for AppendedBlock<'a> {
	type I = std::iter::Copied<std::slice::Iter<'a, (Page<'a>, OOB<'a>)>>;

	fn from_slice(
		slice: &'a [u8],
		page_size: usize,
		oob_size: usize,
		pages_per_block: usize,
	) -> Vec<Self> {
		let pages_section = page_size * pages_per_block;
		let oob_section = oob_size * pages_per_block;
		let block_raw_size = pages_section + oob_section;

		assert!(slice.len() % block_raw_size == 0);

		slice
			.chunks_exact(block_raw_size)
			.map(|c| {
				let pages_in_block = &c[..pages_section];
				let oob_in_block = &c[pages_section..];

				let page_iter = pages_in_block
					.chunks_exact(page_size)
					.map(|p| Page { data: p });
				let oob_iter = oob_in_block.chunks_exact(oob_size).map(|o| OOB { data: o });

				let data = page_iter.zip(oob_iter).collect();

				Self { data }
			})
			.collect()
	}

	fn iter(&'a self) -> Self::I { self.data.iter().copied() }

	fn empty_block(&'a self) -> bool {
		for (page, _) in self.iter() {
			if page.data.iter().copied().any(|x| x != 0xff) {
				return false;
			}
			// if oob.data.iter().cloned().any(|x| x != 0xff) {
			// 	return false;
			// }
		}
		true
	}

	fn bad_block(&'a self) -> bool {
		let num_pages_to_check = 1;
		let byte_offset = 6;

		let mut bad = true;
		for (_, oob) in self.iter().take(num_pages_to_check) {
			if oob.data[byte_offset] == 0xff {
				bad = false;
			}
		}
		bad
	}
}
