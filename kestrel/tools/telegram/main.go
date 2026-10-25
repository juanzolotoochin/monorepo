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
	"mime/multipart"
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
	var caption, videoPath string
	var dryRun, driveOnly, authorizeDrive bool
	flag.StringVar(&caption, "caption", "", "text to post with the Drive link")
	flag.StringVar(&videoPath, "video", "", "send a demo MP4 directly to Telegram")
	flag.BoolVar(&dryRun, "dry-run", false, "build without uploading or posting")
	flag.BoolVar(&driveOnly, "drive-only", false, "upload to Drive without posting to Telegram")
	flag.BoolVar(&authorizeDrive, "authorize-drive", false, "sign in to personal Google Drive and save a refresh token")
	flag.Parse()
	if flag.NArg() != 0 {
		return fmt.Errorf("unexpected arguments: %s", strings.Join(flag.Args(), " "))
	}
	if videoPath != "" && (driveOnly || authorizeDrive) {
		return errors.New("--video cannot be combined with Drive-only or Drive authorization")
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
	credentialsDir := filepath.Join(workspace, ".credentials")
	if authorizeDrive {
		folderID := firstNonEmpty(os.Getenv("GOOGLE_DRIVE_FOLDER_ID"), config.GoogleDriveFolderID)
		if folderID == "" {
			return errors.New("set the Drive folder ID before authorizing")
		}
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
		defer cancel()
		return authorizeUserDrive(ctx, filepath.Join(credentialsDir, "google-oauth-client.json"), filepath.Join(credentialsDir, "google-user-token.json"), folderID)
	}
	token := firstNonEmpty(os.Getenv("TELEGRAM_BOT_TOKEN"), config.TelegramBotToken)
	chatID := firstNonEmpty(os.Getenv("TELEGRAM_CHAT_ID"), config.TelegramChatID)
	if videoPath != "" {
		info, err := os.Stat(videoPath)
		if err != nil {
			return fmt.Errorf("read demo video: %w", err)
		}
		fmt.Printf("Demo video: %s (%.1f MiB)\n", videoPath, float64(info.Size())/(1024*1024))
		if dryRun {
			return nil
		}
		if token == "" || chatID == "" {
			return errors.New("set the Telegram bot token and chat ID in .credentials/game-share.json (or environment variables)")
		}
		if caption == "" {
			caption = strings.TrimSuffix(filepath.Base(videoPath), filepath.Ext(videoPath))
		}
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Minute)
		defer cancel()
		if err := sendTelegramVideo(ctx, http.DefaultClient, "https://api.telegram.org", token, chatID, caption, videoPath); err != nil {
			return err
		}
		fmt.Println("Sent demo video to Telegram channel", chatID)
		return nil
	}
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
	if !dryRun && folderID == "" {
		return errors.New("set the Drive folder ID in .credentials/game-share.json (or GOOGLE_DRIVE_FOLDER_ID)")
	}
	if !dryRun && !driveOnly && (token == "" || chatID == "") {
		return errors.New("set the Telegram bot token and chat ID in .credentials/game-share.json (or environment variables)")
	}
	builds, cleanup, err := buildReleases(context.Background(), workspace)
	if err != nil {
		return err
	}
	defer cleanup()
	if dryRun {
		return nil
	}
	if caption == "" {
		caption = "Kestrel optimized builds"
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Minute)
	defer cancel()
	if accessToken == "" {
		userTokenPath := filepath.Join(credentialsDir, "google-user-token.json")
		if _, statErr := os.Stat(userTokenPath); statErr == nil {
			accessToken, err = userDriveAccessToken(ctx, filepath.Join(credentialsDir, "google-oauth-client.json"), userTokenPath)
			if err != nil {
				return err
			}
		} else if !errors.Is(statErr, os.ErrNotExist) {
			return fmt.Errorf("check Google user token: %w", statErr)
		}
	}
	if accessToken == "" {
		accessToken, err = driveAccessToken(ctx, http.DefaultClient, "", credentialsPath)
	}
	if err != nil {
		return err
	}
	links := make([]string, 0, len(builds))
	for _, build := range builds {
		link, err := uploadDrive(ctx, http.DefaultClient, "https://www.googleapis.com", accessToken, folderID, build.path, build.name)
		if err != nil {
			return fmt.Errorf("upload %s build: %w", build.platform, err)
		}
		fmt.Printf("Uploaded %s to Google Drive: %s\n", build.platform, link)
		links = append(links, build.platform+": "+link)
	}
	if driveOnly {
		return nil
	}
	if err := sendTelegram(ctx, http.DefaultClient, "https://api.telegram.org", token, chatID, caption+"\n"+strings.Join(links, "\n")); err != nil {
		return fmt.Errorf("Drive uploads succeeded but Telegram post failed: %w", err)
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

type releaseBuild struct {
	platform string
	name     string
	path     string
}

func buildReleases(ctx context.Context, workspace string) ([]releaseBuild, func(), error) {
	if runtime.GOOS != "linux" || runtime.GOARCH != "amd64" {
		return nil, nil, errors.New("publishing Linux and Windows x64 builds currently requires a Linux x64 host")
	}
	stagingDir, err := os.MkdirTemp("", "kestrel-releases-*")
	if err != nil {
		return nil, nil, fmt.Errorf("create build staging directory: %w", err)
	}
	cleanup := func() { os.RemoveAll(stagingDir) }
	timestamp := time.Now().UTC().Format("20060102-150405")
	var builds []releaseBuild
	for _, target := range []struct {
		platform  string
		os        string
		extension string
		flags     []string
	}{
		{platform: "Linux x64", os: "linux"},
		{platform: "Windows x64", os: "windows", extension: ".exe", flags: []string{
			"--platforms=//platforms:rust_windows_x86_64",
			"--repo_env=BAZEL_MSVC_RUNTIME_VISUAL_STUDIO_EULA=1",
			"--repo_env=BAZEL_WINDOWS_SDK_EULA=1",
		}},
	} {
		if err := build(ctx, workspace, target.flags...); err != nil {
			cleanup()
			return nil, nil, fmt.Errorf("%s: %w", target.platform, err)
		}
		name := fmt.Sprintf("kestrel-%s-amd64-%s%s", target.os, timestamp, target.extension)
		source := filepath.Join(workspace, "bazel-bin", "kestrel", "app", "demo"+target.extension)
		path := filepath.Join(stagingDir, name)
		if err := copyFile(path, source); err != nil {
			cleanup()
			return nil, nil, fmt.Errorf("stage %s build: %w", target.platform, err)
		}
		info, err := os.Stat(path)
		if err != nil {
			cleanup()
			return nil, nil, err
		}
		fmt.Printf("Built %s (%.1f MiB)\n", name, float64(info.Size())/(1024*1024))
		builds = append(builds, releaseBuild{platform: target.platform, name: name, path: path})
	}
	return builds, cleanup, nil
}

func build(ctx context.Context, workspace string, flags ...string) error {
	args := append([]string{"build", "--config=full_local", "-c", "opt"}, flags...)
	args = append(args, "//kestrel/app:demo")
	cmd := exec.CommandContext(ctx, "bazel", args...)
	cmd.Dir = workspace
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("optimized game build failed: %w", err)
	}
	return nil
}

func copyFile(destination, source string) error {
	in, err := os.Open(source)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.OpenFile(destination, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0755)
	if err != nil {
		return err
	}
	_, copyErr := io.Copy(out, in)
	closeErr := out.Close()
	if copyErr != nil {
		return copyErr
	}
	return closeErr
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
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("start Drive upload: %w", driveResponseError(resp))
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
		return "", fmt.Errorf("upload to Drive: %w", driveResponseError(resp))
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

func driveResponseError(resp *http.Response) error {
	var body struct {
		Error struct {
			Message string `json:"message"`
			Errors  []struct {
				Reason string `json:"reason"`
			} `json:"errors"`
		} `json:"error"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 8192)).Decode(&body); err != nil || body.Error.Message == "" {
		return fmt.Errorf("HTTP %d", resp.StatusCode)
	}
	if len(body.Error.Errors) != 0 && body.Error.Errors[0].Reason != "" {
		return fmt.Errorf("HTTP %d: %s (%s)", resp.StatusCode, body.Error.Message, body.Error.Errors[0].Reason)
	}
	return fmt.Errorf("HTTP %d: %s", resp.StatusCode, body.Error.Message)
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
	return checkTelegramResponse(resp, token)
}

func sendTelegramVideo(ctx context.Context, client *http.Client, baseURL, token, chatID, caption, videoPath string) error {
	video, err := os.Open(videoPath)
	if err != nil {
		return fmt.Errorf("open demo video: %w", err)
	}
	defer video.Close()
	reader, writer := io.Pipe()
	defer reader.Close()
	form := multipart.NewWriter(writer)
	go func() {
		var writeErr error
		for key, value := range map[string]string{"chat_id": chatID, "caption": caption, "supports_streaming": "true"} {
			writeErr = form.WriteField(key, value)
			if writeErr != nil {
				break
			}
		}
		if writeErr == nil {
			var part io.Writer
			part, writeErr = form.CreateFormFile("video", filepath.Base(videoPath))
			if writeErr == nil {
				_, writeErr = io.Copy(part, video)
			}
		}
		if writeErr == nil {
			writeErr = form.Close()
		}
		writer.CloseWithError(writeErr)
	}()
	endpoint := strings.TrimRight(baseURL, "/") + "/bot" + token + "/sendVideo"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, reader)
	if err != nil {
		reader.Close()
		return errors.New("invalid Telegram API URL")
	}
	req.Header.Set("Content-Type", form.FormDataContentType())
	resp, err := client.Do(req)
	if err != nil {
		reader.Close()
		return fmt.Errorf("Telegram video request failed: %v", strings.ReplaceAll(err.Error(), token, "[redacted]"))
	}
	defer resp.Body.Close()
	return checkTelegramResponse(resp, token)
}

func checkTelegramResponse(resp *http.Response, token string) error {
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
