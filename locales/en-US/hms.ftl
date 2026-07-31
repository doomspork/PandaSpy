### PandaSpy — Health Management System (HMS) error descriptions.
###
### `pandaspy-proto::lookup_hms` maps a printer's HMS code to a message id in this
### file. The table maps codes to KEYS, never to English text, so that an error
### the printer reports at 3am reads in the user's language.
###
### Naming convention: `hms-<code with underscores lowercased>`, e.g.
###   hms-0300-0100-0002-0001 = Nozzle temperature is abnormal
###
### Empty for now. Add an entry only alongside a fixture that actually exhibits
### the code — see `.claude/commands/fixture.md`.
