package main

import (
	"bytes"
	"context"
	"crypto"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"errors"
	"flag"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

const driveScope = "https://www.googleapis.com/auth/drive"

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "publish:", err)
		os.Exit(1)
	}
}

func run() error {
	var caption string
	var dryRun bool
	flag.StringVar(&caption, "caption", "", "text to post with the Drive link")
	flag.BoolVar(&dryRun, "dry-run", false, "build without uploading or posting")
	flag.Parse()
	if flag.NArg() != 0 {
		return fmt.Errorf("unexpected arguments: %s", strings.Join(flag.Args(), " "))
	}

	workspace := os.Getenv("BUILD_WORKSPACE_DIRECTORY")
	if workspace == "" {
		var err error
		workspace, err = os.Getwd()
		if err != nil {
			return err
		}
	}
	if _, err := os.Stat(filepath.Join(workspace, "MODULE.bazel")); err != nil {
		return fmt.Errorf("run from the repository root or with bazel run: %w", err)
	}
	config, err := loadConfig(filepath.Join(workspace, ".credentials", "game-share.json"))
	if err != nil {
		return err
	}
	token := firstNonEmpty(os.Getenv("TELEGRAM_BOT_TOKEN"), config.TelegramBotToken)
	chatID := firstNonEmpty(os.Getenv("TELEGRAM_CHAT_ID"), config.TelegramChatID)
	folderID := firstNonEmpty(os.Getenv("GOOGLE_DRIVE_FOLDER_ID"), config.GoogleDriveFolderID)
	accessToken := firstNonEmpty(os.Getenv("GOOGLE_DRIVE_ACCESS_TOKEN"), config.GoogleDriveAccessToken)
	credentialsPath := os.Getenv("GOOGLE_APPLICATION_CREDENTIALS")
	if credentialsPath == "" {
		credentialsFile := firstNonEmpty(config.GoogleServiceAccountFile, "google-drive-service-account.json")
		if filepath.IsAbs(credentialsFile) {
			credentialsPath = credentialsFile
		} else {
			credentialsPath = filepath.Join(workspace, ".credentials", credentialsFile)
		}
	}
	if !dryRun && (token == "" || chatID == "" || folderID == "") {
		return errors.New("set Telegram bot token, chat ID, and Drive folder ID in .credentials/game-share.json (or environment variables)")
	}
	if err := build(context.Background(), workspace); err != nil {
		return err
	}

	extension := ""
	if runtime.GOOS == "windows" {
		extension = ".exe"
	}
	name := fmt.Sprintf("kestrel-%s-%s-%s%s", runtime.GOOS, runtime.GOARCH, time.Now().UTC().Format("20060102-150405"), extension)
	binaryPath := filepath.Join(workspace, "bazel-bin", "kestrel", "app", "demo"+extension)
	info, err := os.Stat(binaryPath)
	if err != nil {
		return err
	}
	fmt.Printf("Built %s (%.1f MiB)\n", binaryPath, float64(info.Size())/(1024*1024))
	if dryRun {
		return nil
	}
	if caption == "" {
		caption = fmt.Sprintf("Kestrel %s/%s optimized build", runtime.GOOS, runtime.GOARCH)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Minute)
	defer cancel()
	accessToken, err = driveAccessToken(ctx, http.DefaultClient, accessToken, credentialsPath)
	if err != nil {
		return err
	}
	link, err := uploadDrive(ctx, http.DefaultClient, "https://www.googleapis.com", accessToken, folderID, binaryPath, name)
	if err != nil {
		return err
	}
	fmt.Println("Uploaded to Google Drive:", link)
	if err := sendTelegram(ctx, http.DefaultClient, "https://api.telegram.org", token, chatID, caption+"\n"+link); err != nil {
		return fmt.Errorf("Drive upload succeeded but Telegram post failed: %w", err)
	}
	fmt.Println("Sent Drive link to Telegram channel", chatID)
	return nil
}

type publishConfig struct {
	GoogleServiceAccountFile string `json:"google_service_account_file"`
	GoogleDriveFolderID      string `json:"google_drive_folder_id"`
	GoogleDriveAccessToken   string `json:"google_drive_access_token"`
	TelegramBotToken         string `json:"telegram_bot_token"`
	TelegramChatID           string `json:"telegram_chat_id"`
}

func loadConfig(path string) (publishConfig, error) {
	var config publishConfig
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return config, nil
	}
	if err != nil {
		return config, fmt.Errorf("read publishing credentials: %w", err)
	}
	if err := json.Unmarshal(data, &config); err != nil {
		return config, fmt.Errorf("parse publishing credentials %s: %w", path, err)
	}
	return config, nil
}

func firstNonEmpty(first, fallback string) string {
	if first != "" {
		return first
	}
	return fallback
}

func build(ctx context.Context, workspace string) error {
	cmd := exec.CommandContext(ctx, "bazel", "build", "--config=full_local", "-c", "opt", "//kestrel/app:demo")
	cmd.Dir = workspace
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("optimized game build failed: %w", err)
	}
	return nil
}

func driveAccessToken(ctx context.Context, client *http.Client, accessToken, credentialsPath string) (string, error) {
	if accessToken != "" {
		return accessToken, nil
	}
	data, err := os.ReadFile(credentialsPath)
	if err != nil {
		return "", fmt.Errorf("read Google credentials %s: %w (place the key there or set GOOGLE_APPLICATION_CREDENTIALS)", credentialsPath, err)
	}
	var credentials struct {
		Type        string `json:"type"`
		ClientEmail string `json:"client_email"`
		PrivateKey  string `json:"private_key"`
		TokenURI    string `json:"token_uri"`
	}
	if err := json.Unmarshal(data, &credentials); err != nil || credentials.Type != "service_account" || credentials.ClientEmail == "" || credentials.TokenURI == "" {
		return "", errors.New("GOOGLE_APPLICATION_CREDENTIALS must contain a service account JSON key")
	}
	block, _ := pem.Decode([]byte(credentials.PrivateKey))
	if block == nil {
		return "", errors.New("Google service account key has invalid PEM data")
	}
	key, err := x509.ParsePKCS8PrivateKey(block.Bytes)
	if err != nil {
		return "", fmt.Errorf("parse Google service account key: %w", err)
	}
	rsaKey, ok := key.(*rsa.PrivateKey)
	if !ok {
		return "", errors.New("Google service account key is not RSA")
	}
	now := time.Now().Unix()
	claims, _ := json.Marshal(map[string]any{"iss": credentials.ClientEmail, "scope": driveScope, "aud": credentials.TokenURI, "iat": now, "exp": now + 3600})
	header := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"RS256","typ":"JWT"}`))
	unsigned := header + "." + base64.RawURLEncoding.EncodeToString(claims)
	hash := sha256.Sum256([]byte(unsigned))
	signature, err := rsa.SignPKCS1v15(rand.Reader, rsaKey, crypto.SHA256, hash[:])
	if err != nil {
		return "", fmt.Errorf("sign Google credentials: %w", err)
	}
	assertion := unsigned + "." + base64.RawURLEncoding.EncodeToString(signature)
	form := url.Values{"grant_type": {"urn:ietf:params:oauth:grant-type:jwt-bearer"}, "assertion": {assertion}}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, credentials.TokenURI, strings.NewReader(form.Encode()))
	if err != nil {
		return "", errors.New("Google credentials have an invalid token URI")
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	resp, err := client.Do(req)
	if err != nil {
		return "", fmt.Errorf("request Google access token: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("Google token endpoint returned HTTP %d", resp.StatusCode)
	}
	var tokenReply struct {
		AccessToken string `json:"access_token"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&tokenReply); err != nil || tokenReply.AccessToken == "" {
		return "", errors.New("Google token endpoint returned no access token")
	}
	return tokenReply.AccessToken, nil
}

func uploadDrive(ctx context.Context, client *http.Client, baseURL, accessToken, folderID, path, name string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil {
		return "", err
	}
	metadata, err := json.Marshal(struct {
		Name    string   `json:"name"`
		Parents []string `json:"parents"`
	}{name, []string{folderID}})
	if err != nil {
		return "", err
	}
	endpoint := strings.TrimRight(baseURL, "/") + "/upload/drive/v3/files?uploadType=resumable&supportsAllDrives=true&fields=id,webViewLink"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(metadata))
	if err != nil {
		return "", err
	}
	req.Header.Set("Content-Type", "application/json; charset=UTF-8")
	req.Header.Set("Authorization", "Bearer "+accessToken)
	req.Header.Set("X-Upload-Content-Type", "application/octet-stream")
	req.Header.Set("X-Upload-Content-Length", fmt.Sprint(info.Size()))
	resp, err := client.Do(req)
	if err != nil {
		return "", fmt.Errorf("start Drive upload: %w", err)
	}
	resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("start Drive upload: HTTP %d", resp.StatusCode)
	}
	location := resp.Header.Get("Location")
	if location == "" {
		return "", errors.New("Drive upload response had no Location header")
	}
	put, err := http.NewRequestWithContext(ctx, http.MethodPut, location, f)
	if err != nil {
		return "", errors.New("Drive returned an invalid upload URL")
	}
	put.ContentLength = info.Size()
	put.Header.Set("Authorization", "Bearer "+accessToken)
	put.Header.Set("Content-Type", "application/octet-stream")
	resp, err = client.Do(put)
	if err != nil {
		return "", fmt.Errorf("upload to Drive: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		return "", fmt.Errorf("upload to Drive: HTTP %d", resp.StatusCode)
	}
	var file struct {
		ID          string `json:"id"`
		WebViewLink string `json:"webViewLink"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&file); err != nil {
		return "", fmt.Errorf("decode Drive upload response: %w", err)
	}
	if file.ID == "" {
		return "", errors.New("Drive upload response had no file ID")
	}
	if file.WebViewLink != "" {
		return file.WebViewLink, nil
	}
	return "https://drive.google.com/file/d/" + url.PathEscape(file.ID) + "/view", nil
}

func sendTelegram(ctx context.Context, client *http.Client, baseURL, token, chatID, message string) error {
	values := url.Values{"chat_id": {chatID}, "text": {message}}
	endpoint := strings.TrimRight(baseURL, "/") + "/bot" + token + "/sendMessage"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, strings.NewReader(values.Encode()))
	if err != nil {
		return errors.New("invalid Telegram API URL")
	}
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	resp, err := client.Do(req)
	if err != nil {
		return fmt.Errorf("Telegram request failed: %v", strings.ReplaceAll(err.Error(), token, "[redacted]"))
	}
	defer resp.Body.Close()
	var reply struct {
		OK          bool   `json:"ok"`
		Description string `json:"description"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&reply); err != nil {
		return fmt.Errorf("Telegram returned HTTP %d with an invalid response", resp.StatusCode)
	}
	if resp.StatusCode != http.StatusOK || !reply.OK {
		return fmt.Errorf("Telegram returned HTTP %d: %s", resp.StatusCode, strings.ReplaceAll(reply.Description, token, "[redacted]"))
	}
	return nil
}
