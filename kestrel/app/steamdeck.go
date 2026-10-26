package main

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strconv"
	"strings"
	"sync/atomic"
	"time"

	"github.com/klauspost/compress/zstd"
	"golang.org/x/crypto/ssh"
	"golang.org/x/crypto/ssh/agent"
	"golang.org/x/crypto/ssh/knownhosts"
)

const remoteDir = "$HOME/.local/opt/kestrel"

var targetPattern = regexp.MustCompile(`^[a-zA-Z_][a-zA-Z0-9_-]*@[a-zA-Z0-9][a-zA-Z0-9._-]*$`)

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "steamdeck:", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) < 2 {
		return errors.New("missing Bazel game or launcher runfile")
	}
	game, err := runfile(args[0])
	if err != nil {
		return err
	}
	launcher, err := runfile(args[1])
	if err != nil {
		return err
	}
	gameArgs := args[2:]
	dryRun := len(gameArgs) > 0 && gameArgs[0] == "--dry-run"
	stop := len(gameArgs) > 0 && gameArgs[0] == "--stop"
	installOnly := len(gameArgs) > 0 && gameArgs[0] == "--install-only"
	steamLaunch := len(gameArgs) > 0 && gameArgs[0] == "--steam"
	if dryRun {
		gameArgs = gameArgs[1:]
	}
	target := os.Getenv("STEAM_DECK_TARGET")
	if target == "" {
		target = "deck@steamdeck.local"
	}
	if !targetPattern.MatchString(target) {
		return errors.New("STEAM_DECK_TARGET must be user@host, for example deck@steamdeck.local")
	}
	if runtime.GOOS != "linux" || runtime.GOARCH != "amd64" {
		return errors.New("the Steam Deck target needs a Linux x86-64 Bazel build host")
	}
	if dryRun {
		fmt.Printf("Would copy the Bazel-built game and launcher to %s:.local/opt/kestrel/\n", target)
		fmt.Printf("Would start Kestrel in the Deck's Desktop Mode session with arguments: %q\n", gameArgs)
		return nil
	}

	username, host, _ := strings.Cut(target, "@")
	config, closeAgent, err := sshConfig(username)
	if err != nil {
		return err
	}
	defer closeAgent()
	fmt.Printf("Connecting to %s...\n", target)
	conn, err := net.DialTimeout("tcp", net.JoinHostPort(host, "22"), 10*time.Second)
	if err != nil {
		return fmt.Errorf("connect to Deck: %w", err)
	}
	if err := conn.SetDeadline(time.Now().Add(15 * time.Second)); err != nil {
		conn.Close()
		return err
	}
	sshConn, chans, reqs, err := ssh.NewClientConn(conn, net.JoinHostPort(host, "22"), config)
	if err != nil {
		conn.Close()
		return fmt.Errorf("authenticate with Deck: %w", err)
	}
	if err := conn.SetDeadline(time.Time{}); err != nil {
		sshConn.Close()
		return err
	}
	client := ssh.NewClient(sshConn, chans, reqs)
	defer client.Close()
	if stop {
		fmt.Println("Stopping Kestrel on the Deck...")
		return remoteRun(client, "systemctl --user stop kestrel-game.service 2>/dev/null || true")
	}
	var steamID uint64
	if steamLaunch {
		steamID, err = steamShortcutID(client)
		if err != nil {
			return err
		}
	}

	if err := remoteRun(client, `mkdir -p "`+remoteDir+`"`); err != nil {
		return fmt.Errorf("create Deck directory: %w", err)
	}
	if err := upload(client, game, remoteDir+"/demo.next", "game", true); err != nil {
		return err
	}
	if err := upload(client, launcher, remoteDir+"/launch.sh.next", "launcher", false); err != nil {
		return err
	}
	fmt.Println("Installing game on the Deck...")
	if err := remoteRun(client, `mv "`+remoteDir+`/demo.next" "`+remoteDir+`/demo" && mv "`+remoteDir+`/launch.sh.next" "`+remoteDir+`/launch.sh" && chmod +x "`+remoteDir+`/demo"`); err != nil {
		return fmt.Errorf("install game: %w", err)
	}
	if installOnly {
		fmt.Println("Installed Kestrel on the Deck at ~/.local/opt/kestrel/demo.")
		return nil
	}
	if steamLaunch {
		fmt.Println("Launching Kestrel through its Steam shortcut...")
		return remoteRun(client, `bash "`+remoteDir+`/launch.sh" --steam `+strconv.FormatUint(steamID, 10))
	}
	command := `bash "` + remoteDir + `/launch.sh"`
	for _, arg := range gameArgs {
		command += " " + shellQuote(arg)
	}
	fmt.Println("Starting Kestrel in the Deck's Desktop Mode session...")
	if err := remoteRun(client, command); err != nil {
		return fmt.Errorf("launch game: %w", err)
	}
	return nil
}

func runfile(path string) (string, error) {
	dir := os.Getenv("RUNFILES_DIR")
	if dir == "" {
		dir = os.Args[0] + ".runfiles"
	}
	file := filepath.Join(dir, path)
	if !filepath.IsLocal(path) {
		return "", fmt.Errorf("invalid Bazel runfile: %q", path)
	}
	if _, err := os.Stat(file); err != nil {
		return "", fmt.Errorf("missing Bazel runfile %s: %w", path, err)
	}
	return file, nil
}

func sshConfig(username string) (*ssh.ClientConfig, func(), error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, nil, err
	}
	knownHosts := filepath.Join(home, ".ssh", "known_hosts")
	hostKeyCallback, err := knownhosts.New(knownHosts)
	if err != nil {
		return nil, nil, fmt.Errorf("read %s (connect once with ssh first): %w", knownHosts, err)
	}
	var auth []ssh.AuthMethod
	closeAgent := func() {}
	if socket := os.Getenv("SSH_AUTH_SOCK"); socket != "" {
		if conn, err := net.Dial("unix", socket); err == nil {
			auth = append(auth, ssh.PublicKeysCallback(agent.NewClient(conn).Signers))
			closeAgent = func() { conn.Close() }
		}
	}
	for _, name := range []string{"id_ed25519", "id_ecdsa", "id_rsa"} {
		key, err := os.ReadFile(filepath.Join(home, ".ssh", name))
		if err != nil {
			continue
		}
		signer, err := ssh.ParsePrivateKey(key)
		if err == nil {
			auth = append(auth, ssh.PublicKeys(signer))
		}
	}
	if len(auth) == 0 {
		closeAgent()
		return nil, nil, errors.New("no SSH key available; use ssh-add or create a default ~/.ssh/id_ed25519 key")
	}
	return &ssh.ClientConfig{
		User:            username,
		Auth:            auth,
		HostKeyCallback: hostKeyCallback,
	}, closeAgent, nil
}

func remoteRun(client *ssh.Client, command string) error {
	session, err := client.NewSession()
	if err != nil {
		return err
	}
	defer session.Close()
	session.Stdout = os.Stdout
	session.Stderr = os.Stderr
	return session.Run(command)
}

func steamShortcutID(client *ssh.Client) (uint64, error) {
	session, err := client.NewSession()
	if err != nil {
		return 0, err
	}
	defer session.Close()
	data, err := session.Output(`for file in "$HOME"/.local/share/Steam/userdata/*/config/shortcuts.vdf; do if [ -f "$file" ]; then cat "$file"; fi; done`)
	if err != nil {
		return 0, fmt.Errorf("read Steam shortcuts: %w", err)
	}
	shortcuts, err := parseSteamShortcuts(data)
	if err != nil {
		return 0, fmt.Errorf("parse Steam shortcuts: %w", err)
	}
	for _, shortcut := range shortcuts {
		if strings.Contains(shortcut.exe, "/.local/opt/kestrel/demo") {
			return uint64(shortcut.appID)<<32 | 0x02000000, nil
		}
	}
	return 0, errors.New("Kestrel is not in Steam yet; add ~/.local/opt/kestrel/demo as a Non-Steam Game on the Deck, then retry --steam")
}

type steamShortcut struct {
	appID uint32
	exe   string
}

// Steam's shortcuts.vdf is binary KeyValues: nested objects, zero-terminated
// names/strings, little-endian integers, and 0x08 object terminators.
func parseSteamShortcuts(data []byte) ([]steamShortcut, error) {
	var shortcuts []steamShortcut
	position := 0
	var object func() (steamShortcut, error)
	readString := func() (string, error) {
		start := position
		for position < len(data) && data[position] != 0 {
			position++
		}
		if position == len(data) {
			return "", io.ErrUnexpectedEOF
		}
		value := string(data[start:position])
		position++
		return value, nil
	}
	object = func() (steamShortcut, error) {
		var shortcut steamShortcut
		for position < len(data) {
			kind := data[position]
			position++
			if kind == 8 {
				return shortcut, nil
			}
			key, err := readString()
			if err != nil {
				return shortcut, err
			}
			switch kind {
			case 0:
				child, err := object()
				if err != nil {
					return shortcut, err
				}
				if child.appID != 0 && child.exe != "" {
					shortcuts = append(shortcuts, child)
				}
			case 1:
				value, err := readString()
				if err != nil {
					return shortcut, err
				}
				if strings.EqualFold(key, "exe") {
					shortcut.exe = value
				}
			case 2, 3, 4, 6:
				if len(data)-position < 4 {
					return shortcut, io.ErrUnexpectedEOF
				}
				if strings.EqualFold(key, "appid") {
					shortcut.appID = binary.LittleEndian.Uint32(data[position:])
				}
				position += 4
			case 7, 9:
				if len(data)-position < 8 {
					return shortcut, io.ErrUnexpectedEOF
				}
				position += 8
			default:
				return shortcut, fmt.Errorf("unsupported Steam shortcut value type %d", kind)
			}
		}
		return shortcut, nil
	}
	_, err := object()
	return shortcuts, err
}

func upload(client *ssh.Client, path, destination, label string, compress bool) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return err
	}
	if compress {
		fmt.Printf("Compressing and copying %s (%.1f MiB before compression)...\n", label, float64(info.Size())/(1024*1024))
	} else {
		fmt.Printf("Copying %s (%.1f MiB)...\n", label, float64(info.Size())/(1024*1024))
	}
	session, err := client.NewSession()
	if err != nil {
		return err
	}
	defer session.Close()
	session.Stderr = os.Stderr
	stdin, err := session.StdinPipe()
	if err != nil {
		return err
	}
	if err := session.Start(uploadCommand(destination, compress)); err != nil {
		return err
	}
	var processed, sent atomic.Int64
	var sink io.Writer = &progressWriter{writer: stdin, copied: &sent}
	var compressor *zstd.Encoder
	if compress {
		compressor, err = zstd.NewWriter(sink, zstd.WithEncoderLevel(zstd.SpeedFastest))
		if err != nil {
			return err
		}
		sink = compressor
	}
	done := make(chan struct{})
	finished := make(chan struct{})
	go func() {
		showProgress(label, info.Size(), &processed, &sent, done)
		close(finished)
	}()
	_, copyErr := io.Copy(&progressWriter{writer: sink, copied: &processed}, file)
	if compressor != nil {
		if err := compressor.Close(); copyErr == nil {
			copyErr = err
		}
	}
	closeErr := stdin.Close()
	waitErr := session.Wait()
	close(done)
	<-finished
	if copyErr != nil {
		return fmt.Errorf("copy %s: %w", label, copyErr)
	}
	if closeErr != nil {
		return fmt.Errorf("finish %s copy: %w", label, closeErr)
	}
	if waitErr != nil {
		return fmt.Errorf("save %s on Deck: %w", label, waitErr)
	}
	return nil
}

type progressWriter struct {
	writer io.Writer
	copied *atomic.Int64
}

func (p *progressWriter) Write(data []byte) (int, error) {
	n, err := p.writer.Write(data)
	p.copied.Add(int64(n))
	return n, err
}

func showProgress(label string, total int64, processed, sent *atomic.Int64, done <-chan struct{}) {
	start := time.Now()
	ticker := time.NewTicker(time.Second)
	defer ticker.Stop()
	printProgress := func(final bool) {
		bytes := processed.Load()
		wireBytes := sent.Load()
		percent := 100.0
		if total > 0 {
			percent = 100 * float64(bytes) / float64(total)
		}
		elapsed := time.Since(start).Seconds()
		if elapsed < 0.001 {
			elapsed = 0.001
		}
		end := "\r"
		if final {
			end = "\n"
		}
		fmt.Fprintf(os.Stderr, "%s: %5.1f%% (%.1f/%.1f MiB processed, %.1f MiB sent, %.1f MiB/s SSH payload)%s", label, percent, float64(bytes)/(1024*1024), float64(total)/(1024*1024), float64(wireBytes)/(1024*1024), float64(wireBytes)/(1024*1024)/elapsed, end)
	}
	for {
		select {
		case <-ticker.C:
			printProgress(false)
		case <-done:
			printProgress(true)
			return
		}
	}
}

func shellQuote(arg string) string {
	return "'" + strings.ReplaceAll(arg, "'", `'\''`) + "'"
}

func uploadCommand(destination string, compressed bool) string {
	writer := "cat"
	if compressed {
		writer = "zstd -d -q"
	}
	return `rm -f "` + destination + `" && ` + writer + ` > "` + destination + `"`
}
