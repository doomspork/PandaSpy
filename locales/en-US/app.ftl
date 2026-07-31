### Spool — application shell strings.
###
### This file is consumed by BOTH sides of the app: fluent-rs builds the tray
### menu and OS notifications from it, and @fluent/bundle builds the window UI
### from the very same file. There is no second copy to keep in sync.
###
### en-US is the reference locale. `cargo xtask locale-check` fails if any other
### locale is missing a key defined here (or defines one that is not).

## Branding

# The product name. A proper noun — left as "Spool" in every locale. It is a
# term rather than a message so that translations can inflect the words around
# it without the name itself ever being retyped.
-brand-name = Spool

## Application shell

window-title = { -brand-name }

## Tray menu

tray-show = Show { -brand-name }
tray-quit = Quit
