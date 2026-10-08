### PandaSpy — teksty powłoki aplikacji.
###
### Ten plik jest używany po obu stronach aplikacji: fluent-rs buduje z niego
### menu w zasobniku i powiadomienia systemowe, a @fluent/bundle interfejs okna.

## Marka

# Nazwa własna — nie tłumaczymy jej.
-brand-name = PandaSpy

## Powłoka aplikacji

window-title = { -brand-name }

## Menu w zasobniku

tray-show = Pokaż { -brand-name }
tray-quit = Zakończ

## Nawigacja okna

nav-add-printer = Dodaj drukarkę
nav-settings = Ustawienia
nav-back = Wstecz

## Wspólne

common-cancel = Anuluj

## Lista drukarek

printer-list-empty-title = Brak drukarek
printer-list-empty-body = Dodaj drukarkę, aby zacząć obserwować ją stąd.
printer-list-empty-cta = Dodaj drukarkę

## Stan połączenia

connection-status = { $status ->
    [disconnected] Rozłączono
    [connecting] Łączenie…
    [handshaking] Uwierzytelnianie…
    [connected] Połączono
    [failed] Połączenie nieudane
   *[unknown] Nieznany
}

connection-reason = { $reason ->
    [wrong-access-code] Nieprawidłowy kod dostępu
    [unreachable] Drukarka nieosiągalna
    [tls] Uzgadnianie TLS nie powiodło się
    [certificate-changed] Certyfikat się zmienił
    [protocol] Nieoczekiwana odpowiedź drukarki
    [connection-closed] Połączenie zostało zamknięte
   *[unknown] Nieznany błąd
}

## Karta drukarki

print-status = { $status ->
    [idle] Bezczynna
    [preparing] Przygotowanie
    [printing] Drukowanie
    [paused] Wstrzymano
    [finished] Zakończono
    [failed] Niepowodzenie
   *[unknown] Nieznany
}

card-progress-percent = { $percent }%
card-layer = Warstwa { $layer } / { $total }

card-print-error = Błąd druku: { $message }

# Accessible names for the popover's per-printer controls.
card-expand-named = Pokaż { $name }
card-collapse-named = Ukryj { $name }
card-remove-named = Usuń { $name }

# The short state label beside a printer's name; shown in capitals.
chip = { $chip ->
    [run] Druk
    [pause] Pauza
    [warn] Uwaga
    [fail] Błąd
    [auth] Kod
    [idle] Wolna
    [done] Gotowe
    [wait] Czekam
   *[off] Offline
}

# Labels of the strip above the list when there is more than one printer.
summary = { $field ->
    [printers] Drukarki
    [printing] Drukuje
    [attention] Uwaga
   *[next-done] Najbliższa
}

detail-layer = Warstwa
detail-progress = Postęp
detail-remaining = Pozostało
detail-waiting = Czekam na pierwszy raport z drukarki…

readout = { $which ->
    [nozzle] Dysza
    [bed] Stół
   *[chamber] Komora
}

card-remove-confirm-title = Usunąć { $name }?
card-remove-confirm-body = PandaSpy przestanie obserwować tę drukarkę i zapomni zapisany kod dostępu.
card-remove-confirm-confirm = Usuń

## AMS

# `number` is the unit's id + 1. HT units number from 129, so the type
# leads and the raw number follows in brackets.
ams-unit-name = { $kind ->
    [lite] AMS Lite { $number }
    [pro2] AMS 2 Pro { $number }
    [ht] AMS HT (AMS { $number })
   *[other] AMS { $number }
}

ams-humidity = Wilgotność { $percent }%
ams-temperature = { $temp }°
ams-tray-empty = Pusty
ams-tray-remaining = Pozostało { $percent }%
ams-active-badge = Podawanie

## HMS / błędy stanu

hms-severity = { $severity ->
    [fatal] Krytyczny
    [serious] Poważny
    [common] Ostrzeżenie
    [info] Informacja
   *[unknown] Powiadomienie
}

hms-code-only = Kod { $code }
hms-learn-more = Dowiedz się więcej

## Dodawanie drukarki

add-printer-title = Dodaj drukarkę
add-printer-tab-discovered = Znalezione
add-printer-tab-manual = Ręcznie
add-printer-tab-studio = Bambu Studio

add-printer-scanning = Skanowanie…
add-printer-rescan = Skanuj ponownie
add-printer-already-added = Już dodano
add-printer-use = Użyj

discover-found-count = { $count ->
    [0] Nie znaleziono drukarek
    [one] Znaleziono { $count } drukarkę
    [few] Znaleziono { $count } drukarki
   *[many] Znaleziono { $count } drukarek
}

discover-verdict-empty = { $verdict ->
    [NoUsableInterface] Nie znaleziono żadnego interfejsu sieciowego. Sprawdź połączenie sieciowe i spróbuj ponownie.
    [PermissionDenied] PandaSpy potrzebuje uprawnień, aby widzieć urządzenia w Twojej sieci lokalnej. Sprawdź ustawienia prywatności systemu i spróbuj ponownie.
    [NoResponse] Żadna drukarka nie odpowiedziała. Upewnij się, że drukarka jest włączona i podłączona do tej samej sieci.
   *[unknown] Nie znaleziono żadnych drukarek.
}

add-printer-manual-serial = Numer seryjny
add-printer-manual-address = Adres IP
add-printer-manual-access-code = Kod dostępu
add-printer-manual-nickname = Nazwa własna (opcjonalnie)
add-printer-manual-access-code-hint = { $hasKeyring ->
    [yes] Znajdziesz go na ekranie drukarki w Ustawienia → WLAN. PandaSpy przechowuje go w { $keyring }, nigdy w pliku listy drukarek.
   *[no] Znajdziesz go na ekranie drukarki w Ustawienia → WLAN. PandaSpy przechowuje go bezpiecznie, nigdy w pliku listy drukarek.
}
add-printer-manual-submit = Dodaj drukarkę
add-printer-manual-error-required = Podaj numer seryjny i kod dostępu.
add-printer-manual-error = Nie udało się dodać drukarki: { $message }

add-printer-studio-import = Importuj z Bambu Studio
add-printer-studio-importing = Szukanie drukarek z Bambu Studio…
add-printer-studio-empty = Nie znaleziono jeszcze żadnych drukarek w Bambu Studio.
add-printer-studio-use = Użyj
add-printer-studio-add = Dodaj
add-printer-studio-not-found = Nie znaleziono w tej sieci

## Monit o zaufanie certyfikatowi

trust-title = Certyfikat tej drukarki się zmienił
trust-body = Certyfikat przypięty przez PandaSpy dla tej drukarki nie zgadza się już z tym, który właśnie przedstawiła. Ponowne flashowanie i ktoś podszywający się w Twojej sieci wyglądają stąd identycznie — porównaj poniższy odcisk z tym widocznym na ekranie drukarki, zanim podejmiesz decyzję.
trust-pinned-label = Wcześniej przypięty
trust-presented-label = Przedstawiony teraz
trust-accept = Zgadza się — zaufaj mu
trust-reject = Zablokuj nadal
trust-error = Nie udało się zapisać decyzji: { $message }

## Ustawienia

settings-title = Ustawienia
settings-language = Język
settings-language-system = Zgodnie z systemem
settings-launch-at-login = Uruchamiaj przy starcie systemu
settings-printers = Drukarki
settings-move-up = Przesuń { $name } w górę
settings-move-down = Przesuń { $name } w dół
settings-printers-error = Nie udało się zaktualizować drukarek: { $message }

settings-secrets = { $backend ->
    [os-keyring] Kody dostępu są przechowywane w systemowym { $keyring }.
    [encrypted-file] Brak systemowego magazynu kluczy — kody dostępu są przechowywane w zaszyfrowanym pliku chronionym Twoim loginem.
   *[unknown] Kody dostępu są przechowywane w { $keyring }.
}

settings-save-error = Nie udało się zapisać ustawień: { $message }

## Baner aktualizacji

update-available = Dostępna jest wersja { $version } aplikacji { -brand-name }.
update-install = Zainstaluj i uruchom ponownie
update-installing = Instalowanie…
update-install-error = Aktualizacja nie powiodła się: { $message }
update-dismiss = Odrzuć
