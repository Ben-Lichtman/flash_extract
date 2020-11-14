use std::{
	convert::TryInto,
	fs::{read, File, OpenOptions},
	io::Write,
};

use structopt::StructOpt;

use flash_extract::block::{AppendedBlock, Block, InterlacedBlock};

#[derive(StructOpt)]
struct Opt {
	input: String,
	output: String,
	output_oob: String,
	#[structopt(default_value = "512")]
	page_data_size: usize,
	#[structopt(default_value = "16")]
	page_oob_size: usize,
	#[structopt(default_value = "32")]
	pages_per_block: usize,
	#[structopt(short, long)]
	appened_blocks: bool,
}

fn find_error(calculated_ecc: u64, oob_ecc: u64) -> Option<(usize, usize)> {
	let xor_ecc = calculated_ecc ^ oob_ecc;

	// println!("XOR:\t{:06x}\t{:024b}", xor_ecc, xor_ecc);

	if xor_ecc == 0 || xor_ecc.count_ones() == 1 {
		return None;
	}

	if xor_ecc.count_ones() == 10 {
		return None;
	}

	let _ = ((xor_ecc >> 0 & 1) << 0)
		| ((xor_ecc >> 2 & 1) << 1)
		| ((xor_ecc >> 4 & 1) << 2)
		| ((xor_ecc >> 6 & 1) << 3)
		| ((xor_ecc >> 8 & 1) << 4)
		| ((xor_ecc >> 10 & 1) << 5)
		| ((xor_ecc >> 12 & 1) << 6)
		| ((xor_ecc >> 14 & 1) << 7)
		| ((xor_ecc >> 16 & 1) << 8)
		| ((xor_ecc >> 18 & 1) << 9)
		| ((xor_ecc >> 20 & 1) << 10)
		| ((xor_ecc >> 22 & 1) << 12);

	let xor_compressed = ((xor_ecc >> 1 & 1) << 0)
		| ((xor_ecc >> 3 & 1) << 1)
		| ((xor_ecc >> 5 & 1) << 2)
		| ((xor_ecc >> 7 & 1) << 3)
		| ((xor_ecc >> 9 & 1) << 4)
		| ((xor_ecc >> 11 & 1) << 5)
		| ((xor_ecc >> 13 & 1) << 6)
		| ((xor_ecc >> 15 & 1) << 7)
		| ((xor_ecc >> 17 & 1) << 8)
		| ((xor_ecc >> 19 & 1) << 9)
		| ((xor_ecc >> 21 & 1) << 10)
		| ((xor_ecc >> 23 & 1) << 11);

	let byte = xor_compressed / 8;
	let bit = xor_compressed % 8;
	Some((byte as usize, bit as usize))
}

fn do_interlaced(contents: &[u8], opt: &Opt, output_file: &mut File, output_oob: &mut File) {
	let blocks = InterlacedBlock::from_slice(
		&contents,
		opt.page_data_size,
		opt.page_oob_size,
		opt.pages_per_block,
	);
	println!("Got {:#x} blocks", blocks.len());
	println!("Got {:#x} pages", blocks.len() * opt.pages_per_block);
	println!("");

	for (index, block) in blocks.iter().enumerate() {
		println!("=> Block: {:#x}", index);

		// // Write OOB
		// for (_, oob) in block.iter() {
		// 	let oob_data = oob.data;
		// 	output_oob.write_all(oob_data).unwrap();
		// }

		// Check for empty / bad block
		if block.bad_block() || block.empty_block() {
			// println!("Skipping block {:#}", index);
			continue;
		}

		for (_, (page, oob)) in block.iter().enumerate() {
			// println!("=> Page {:#x}", index);

			// Do ECC calculations

			let calculated_ecc = (page.calc_ecc() >> 1) & 0xffffff;

			// Write OOB
			let oob_data = oob.data;
			output_oob.write_all(oob_data).unwrap();

			let oob_ecc =
				0x00ffffff & u32::from_le_bytes(oob_data[6..10].try_into().unwrap()) as u64;

			// println!("\t\t v v v v v v v v 8 4 2 1");
			// println!("Calc:\t{:06x}\t{:024b}", calculated_ecc, calculated_ecc);
			// println!("File:\t{:06x}\t{:024b}", oob_ecc, oob_ecc);

			let err = find_error(calculated_ecc, oob_ecc);

			match err {
				Some((byte, bit)) => {
					println!("Correcting byte {:#x} bit {}", byte, bit);
					let page_data = page.data;
					output_file.write_all(&page_data[..byte]).unwrap();
					// Correct error
					let err_byte = page_data[byte] ^ (0x80 >> bit);
					output_file.write_all(&[err_byte]).unwrap();
					output_file.write_all(&page_data[byte + 1..]).unwrap();
				}
				None => {
					// Write Block
					let page_data = page.data;
					output_file.write_all(page_data).unwrap();
				}
			}
		}

		// // Print parity info
		// for (i, ((_, oob), parity)) in block.iter().zip(parity.iter()).enumerate() {
		// 	println!("Page {}", i);
		// 	println!("Parity: {:?}", parity);
		// 	println!("OOB: {:x?}", oob.data);
		// }
	}
}

fn do_appended(contents: &[u8], opt: &Opt, output_file: &mut File, output_oob: &mut File) {
	let blocks = AppendedBlock::from_slice(
		&contents,
		opt.page_data_size,
		opt.page_oob_size,
		opt.pages_per_block,
	);
	println!("Got {:#x} blocks", blocks.len());
	println!("Got {:#x} pages", blocks.len() * opt.pages_per_block);
	println!("");

	for (index, block) in blocks.iter().enumerate() {
		// Write OOB
		for (_, oob) in block.iter() {
			let oob_data = oob.data;
			output_oob.write_all(oob_data).unwrap();
		}

		// Check for empty / bad block
		if block.bad_block() || block.empty_block() {
			println!("Skipping block {:#x}", index);
			continue;
		}

		// let parity = block.parity();

		// // Write OOB
		// for (_, oob) in block.iter() {
		// 	let oob_data = oob.data;
		// 	output_oob.write_all(oob_data).unwrap();
		// }

		// Write Block
		for (page, _) in block.iter() {
			let page_data = page.data;
			output_file.write_all(page_data).unwrap();
		}

		// // Print parity info
		// for (i, ((_, oob), parity)) in block.iter().zip(parity.iter()).enumerate() {
		// 	println!("Page {}", i);
		// 	println!("Parity: {:?}", parity);
		// 	println!("OOB: {:x?}", oob.data);
		// }
	}
}

fn main() {
	let opt = Opt::from_args();

	println!("Page Size: {:#x}", opt.page_data_size);
	println!("Page OOB: {:#x}", opt.page_oob_size);
	println!("Pages per block: {:#x}", opt.pages_per_block);
	println!(
		"Raw page size: {:#x}",
		opt.page_data_size + opt.page_oob_size
	);
	println!(
		"Raw block size: {:#x}",
		(opt.page_data_size + opt.page_oob_size) * opt.pages_per_block
	);

	let contents = read(&opt.input).unwrap();
	println!("File size: {:#x}", contents.len());
	println!("");

	let mut output = OpenOptions::new()
		.create(true)
		.write(true)
		.truncate(true)
		.open(&opt.output)
		.unwrap();

	let mut output_oob = OpenOptions::new()
		.create(true)
		.write(true)
		.truncate(true)
		.open(&opt.output_oob)
		.unwrap();

	match opt.appened_blocks {
		false => do_interlaced(&contents, &opt, &mut output, &mut output_oob),
		true => do_appended(&contents, &opt, &mut output, &mut output_oob),
	}
}
