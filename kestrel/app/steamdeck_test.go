package main

import (
	"encoding/binary"
	"testing"
)

func TestParseSteamShortcuts(t *testing.T) {
	data := []byte{0}
	data = append(data, []byte("shortcuts\x00")...)
	data = append(data, 0)
	data = append(data, []byte("0\x00")...)
	data = append(data, 2)
	data = append(data, []byte("appid\x00")...)
	data = binary.LittleEndian.AppendUint32(data, 0x8ca6cc85)
	data = append(data, 1)
	data = append(data, []byte("Exe\x00\"/home/deck/.local/opt/kestrel/demo\"\x00")...)
	data = append(data, 8, 8)
	shortcuts, err := parseSteamShortcuts(data)
	if err != nil {
		t.Fatal(err)
	}
	if len(shortcuts) != 1 || shortcuts[0].appID != 0x8ca6cc85 || shortcuts[0].exe != `"/home/deck/.local/opt/kestrel/demo"` {
		t.Fatalf("unexpected shortcuts: %+v", shortcuts)
	}
}

func TestShellQuote(t *testing.T) {
	for _, tc := range []struct {
		arg  string
		want string
	}{
		{"moonlit-glade", "'moonlit-glade'"},
		{"a b", "'a b'"},
		{"don't", "'don'\\''t'"},
		{"", "''"},
	} {
		if got := shellQuote(tc.arg); got != tc.want {
			t.Errorf("shellQuote(%q) = %q, want %q", tc.arg, got, tc.want)
		}
	}
}

func TestTargetPattern(t *testing.T) {
	for _, target := range []string{"deck@steamdeck.local", "deck@192.168.1.10"} {
		if !targetPattern.MatchString(target) {
			t.Errorf("rejected valid target %q", target)
		}
	}
	for _, target := range []string{"steamdeck.local", "deck@host;reboot", "-oProxyCommand=bad@host"} {
		if targetPattern.MatchString(target) {
			t.Errorf("accepted invalid target %q", target)
		}
	}
}

func TestUploadCommandReplacesReadOnlyStagingFile(t *testing.T) {
	want := `rm -f "$HOME/.local/opt/kestrel/demo.next" && cat > "$HOME/.local/opt/kestrel/demo.next"`
	if got := uploadCommand(remoteDir+"/demo.next", false); got != want {
		t.Fatalf("uploadCommand() = %q, want %q", got, want)
	}
	wantCompressed := `rm -f "$HOME/.local/opt/kestrel/demo.next" && zstd -d -q > "$HOME/.local/opt/kestrel/demo.next"`
	if got := uploadCommand(remoteDir+"/demo.next", true); got != wantCompressed {
		t.Fatalf("uploadCommand(compressed) = %q, want %q", got, wantCompressed)
	}
}
