// Fetches license texts from the SPDX license list. Apache-2.0 ships with the
// template, so the default path needs neither network nor a prompt.
let license = variable::get("license");

if license != "Apache-2.0" {
	// Rhai's `replace` mutates in place and returns nothing.
	license.replace(" AND ", " OR ");
	let ids = license.split(" OR ");
	let year = system::date().year.to_string();
	let author = variable::get("author");

	for id in ids {
		let url = "https://raw.githubusercontent.com/spdx/license-list-data/main/text/" + id + ".txt";
		let text = system::command("curl", ["-fsSL", url]);
		text.replace("<year>", year);
		text.replace("<copyright holders>", author);

		file::write(if ids.len() == 1 { "LICENSE" } else { "LICENSE-" + id }, text + "\n");
	}
}
