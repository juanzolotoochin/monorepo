package main

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestUploadDriveThenTelegram(t *testing.T) {
	dir := t.TempDir()
	binaryPath := filepath.Join(dir, "kestrel")
	if err := os.WriteFile(binaryPath, []byte("game binary"), 0755); err != nil {
		t.Fatal(err)
	}
	var drive, telegram bool
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/upload/drive/v3/files":
			if r.Method != http.MethodPost || r.URL.Query().Get("uploadType") != "resumable" {
				t.Errorf("unexpected Drive initiation: %s", r.URL)
			}
			if r.Header.Get("Authorization") != "Bearer access" {
				t.Error("Drive initiation lacked authorization")
			}
			var metadata struct {
				Name    string   `json:"name"`
				Parents []string `json:"parents"`
			}
			if err := json.NewDecoder(r.Body).Decode(&metadata); err != nil {
				t.Error(err)
			}
			if metadata.Name != "kestrel-linux-amd64" || len(metadata.Parents) != 1 || metadata.Parents[0] != "folder" {
				t.Errorf("wrong Drive metadata: %+v", metadata)
			}
			w.Header().Set("Location", "http://"+r.Host+"/upload/session")
		case "/upload/session":
			body, _ := io.ReadAll(r.Body)
			if r.Method != http.MethodPut || string(body) != "game binary" || r.ContentLength != int64(len(body)) {
				t.Errorf("wrong Drive upload: %s, %q", r.Method, body)
			}
			if r.Header.Get("Authorization") != "Bearer access" {
				t.Error("Drive upload lacked authorization")
			}
			drive = true
			w.WriteHeader(http.StatusCreated)
			io.WriteString(w, `{"id":"file123","webViewLink":"https://drive.google.com/file/d/file123/view"}`)
		case "/bottoken/sendMessage":
			if !drive {
				t.Error("Telegram called before Drive upload")
			}
			if err := r.ParseForm(); err != nil {
				t.Error(err)
			}
			if r.Form.Get("chat_id") != "@devgroup" || !strings.Contains(r.Form.Get("text"), "file123") {
				t.Errorf("wrong Telegram message: %v", r.Form)
			}
			telegram = true
			io.WriteString(w, `{"ok":true}`)
		default:
			t.Errorf("unexpected request: %s", r.URL)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer server.Close()
	link, err := uploadDrive(context.Background(), server.Client(), server.URL, "access", "folder", binaryPath, "kestrel-linux-amd64")
	if err != nil || link != "https://drive.google.com/file/d/file123/view" {
		t.Fatalf("Drive upload: %q, %v", link, err)
	}
	if err := sendTelegram(context.Background(), server.Client(), server.URL, "token", "@devgroup", "Build: "+link); err != nil {
		t.Fatal(err)
	}
	if !telegram {
		t.Fatal("Telegram was not called")
	}
}

func TestTelegramErrorRedactsToken(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusForbidden)
		io.WriteString(w, `{"ok":false,"description":"bad token secret-token"}`)
	}))
	defer server.Close()
	err := sendTelegram(context.Background(), server.Client(), server.URL, "secret-token", "@devgroup", "test")
	if err == nil || strings.Contains(err.Error(), "secret-token") {
		t.Fatalf("token exposed in error: %v", err)
	}
}

func TestServiceAccountToken(t *testing.T) {
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := r.ParseForm(); err != nil {
			t.Error(err)
		}
		parts := strings.Split(r.Form.Get("assertion"), ".")
		if len(parts) != 3 {
			t.Error("expected signed JWT")
		} else {
			claims, err := base64.RawURLEncoding.DecodeString(parts[1])
			if err != nil || !strings.Contains(string(claims), driveScope) {
				t.Errorf("wrong JWT claims: %s, %v", claims, err)
			}
		}
		io.WriteString(w, `{"access_token":"issued-token"}`)
	}))
	defer server.Close()
	credentials := map[string]string{
		"type": "service_account", "client_email": "publisher@example.com", "token_uri": server.URL,
		"private_key": string(pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: mustPKCS8(t, key)})),
	}
	data, err := json.Marshal(credentials)
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(t.TempDir(), "credentials.json")
	if err := os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("GOOGLE_APPLICATION_CREDENTIALS", path)
	t.Setenv("GOOGLE_DRIVE_ACCESS_TOKEN", "")
	token, err := driveAccessToken(context.Background(), server.Client())
	if err != nil || token != "issued-token" {
		t.Fatalf("access token: %q, %v", token, err)
	}
}

func mustPKCS8(t *testing.T, key *rsa.PrivateKey) []byte {
	t.Helper()
	data, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		t.Fatal(err)
	}
	return data
}
