package main

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"

	"golang.org/x/oauth2"
)

func TestUserDriveTokenRefresh(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := r.ParseForm(); err != nil {
			t.Error(err)
		}
		if r.Form.Get("grant_type") != "refresh_token" || r.Form.Get("refresh_token") != "saved-refresh" {
			t.Errorf("unexpected token request: %v", r.Form)
		}
		w.Header().Set("Content-Type", "application/json")
		w.Write([]byte(`{"access_token":"fresh-access","expires_in":3600,"token_type":"Bearer"}`))
	}))
	defer server.Close()
	dir := t.TempDir()
	clientPath := filepath.Join(dir, "client.json")
	clientJSON, _ := json.Marshal(map[string]any{"installed": map[string]string{
		"client_id": "client-id", "client_secret": "client-secret",
		"auth_uri": server.URL + "/auth", "token_uri": server.URL,
	}})
	if err := os.WriteFile(clientPath, clientJSON, 0600); err != nil {
		t.Fatal(err)
	}
	config, err := loadOAuthConfig(clientPath)
	if err != nil {
		t.Fatal(err)
	}
	if len(config.Scopes) != 1 || config.Scopes[0] != userDriveScope {
		t.Fatalf("unexpected OAuth scopes: %v, %v", config, err)
	}
	tokenPath := filepath.Join(dir, "token.json")
	if err := saveUserToken(tokenPath, &oauth2.Token{
		AccessToken: "expired-access", RefreshToken: "saved-refresh", Expiry: time.Now().Add(-time.Hour),
	}); err != nil {
		t.Fatal(err)
	}
	accessToken, err := userDriveAccessToken(context.Background(), clientPath, tokenPath)
	if err != nil || accessToken != "fresh-access" {
		t.Fatalf("refreshed token: %q, %v", accessToken, err)
	}
	data, err := os.ReadFile(tokenPath)
	if err != nil {
		t.Fatal(err)
	}
	var saved oauth2.Token
	if err := json.Unmarshal(data, &saved); err != nil || saved.AccessToken != "fresh-access" || saved.RefreshToken != "saved-refresh" {
		t.Fatalf("saved refreshed token: %+v, %v", saved, err)
	}
	info, err := os.Stat(tokenPath)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != 0600 {
		t.Fatalf("token file permissions: %v", info.Mode())
	}
}

func TestContainsID(t *testing.T) {
	if !containsID("other, target ,third", "target") || containsID("other,targeted", "target") {
		t.Fatal("folder selection must match the configured ID exactly")
	}
}
