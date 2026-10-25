package main

import (
	"context"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"

	"golang.org/x/oauth2"
)

const userDriveScope = "https://www.googleapis.com/auth/drive.file"

func loadOAuthConfig(path string) (*oauth2.Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read Google OAuth client %s: %w", path, err)
	}
	var file struct {
		Installed struct {
			ClientID     string `json:"client_id"`
			ClientSecret string `json:"client_secret"`
			AuthURI      string `json:"auth_uri"`
			TokenURI     string `json:"token_uri"`
		} `json:"installed"`
	}
	if err := json.Unmarshal(data, &file); err != nil {
		return nil, fmt.Errorf("parse Google OAuth client: %w", err)
	}
	c := file.Installed
	if c.ClientID == "" || c.AuthURI == "" || c.TokenURI == "" {
		return nil, errors.New("Google OAuth client JSON must be for a Desktop app")
	}
	return &oauth2.Config{
		ClientID: c.ClientID, ClientSecret: c.ClientSecret,
		Scopes:   []string{userDriveScope},
		Endpoint: oauth2.Endpoint{AuthURL: c.AuthURI, TokenURL: c.TokenURI},
	}, nil
}

func authorizeUserDrive(ctx context.Context, clientPath, tokenPath, folderID string) error {
	config, err := loadOAuthConfig(clientPath)
	if err != nil {
		return err
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return fmt.Errorf("start Google sign-in callback: %w", err)
	}
	config.RedirectURL = "http://" + listener.Addr().String() + "/"
	stateBytes := make([]byte, 32)
	if _, err := rand.Read(stateBytes); err != nil {
		listener.Close()
		return err
	}
	state := base64.RawURLEncoding.EncodeToString(stateBytes)
	verifier := oauth2.GenerateVerifier()
	type callbackResult struct {
		code string
		err  error
	}
	result := make(chan callbackResult, 1)
	server := &http.Server{Handler: http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/" || r.URL.Query().Get("state") != state {
			http.Error(w, "Invalid Google sign-in response", http.StatusBadRequest)
			return
		}
		if r.URL.Query().Get("error") != "" {
			select {
			case result <- callbackResult{err: fmt.Errorf("Google sign-in was declined: %s", r.URL.Query().Get("error"))}:
			default:
			}
			http.Error(w, "Google Drive authorization was not completed", http.StatusBadRequest)
			return
		}
		if r.URL.Query().Get("code") == "" || !containsID(r.URL.Query().Get("picked_file_ids"), folderID) {
			select {
			case result <- callbackResult{err: errors.New("choose the configured Drive folder in the Google Picker")}:
			default:
			}
			http.Error(w, "Choose the configured Drive folder and try again", http.StatusBadRequest)
			return
		}
		fmt.Fprintln(w, "Google Drive authorization complete. You can close this tab.")
		select {
		case result <- callbackResult{code: r.URL.Query().Get("code")}:
		default:
		}
	})}
	go server.Serve(listener)
	defer server.Shutdown(context.Background())
	url := config.AuthCodeURL(state,
		oauth2.AccessTypeOffline,
		oauth2.S256ChallengeOption(verifier),
		oauth2.SetAuthURLParam("prompt", "consent"),
		oauth2.SetAuthURLParam("trigger_onepick", "true"),
		oauth2.SetAuthURLParam("allow_folder_selection", "true"),
		oauth2.SetAuthURLParam("file_ids", folderID),
	)
	fmt.Println("Open this URL in your browser to authorize Google Drive:")
	fmt.Println(url)
	var callback callbackResult
	select {
	case callback = <-result:
	case <-ctx.Done():
		return fmt.Errorf("Google sign-in timed out: %w", ctx.Err())
	}
	if callback.err != nil {
		return callback.err
	}
	token, err := config.Exchange(ctx, callback.code, oauth2.VerifierOption(verifier))
	if err != nil {
		return fmt.Errorf("exchange Google sign-in code: %w", err)
	}
	if token.RefreshToken == "" {
		return errors.New("Google did not return a refresh token; revoke this app's access and authorize again")
	}
	if err := saveUserToken(tokenPath, token); err != nil {
		return err
	}
	fmt.Println("Saved Google Drive authorization to", tokenPath)
	return nil
}

func containsID(csvIDs, want string) bool {
	for _, id := range strings.Split(csvIDs, ",") {
		if strings.TrimSpace(id) == want {
			return true
		}
	}
	return false
}

func userDriveAccessToken(ctx context.Context, clientPath, tokenPath string) (string, error) {
	config, err := loadOAuthConfig(clientPath)
	if err != nil {
		return "", err
	}
	data, err := os.ReadFile(tokenPath)
	if err != nil {
		return "", fmt.Errorf("read Google user token: %w", err)
	}
	var saved oauth2.Token
	if err := json.Unmarshal(data, &saved); err != nil || saved.RefreshToken == "" {
		return "", errors.New("Google user token is invalid; run --authorize-drive again")
	}
	updated, err := config.TokenSource(ctx, &saved).Token()
	if err != nil {
		return "", fmt.Errorf("refresh Google user token (run --authorize-drive again if revoked): %w", err)
	}
	if updated.AccessToken != saved.AccessToken || updated.RefreshToken != saved.RefreshToken || !updated.Expiry.Equal(saved.Expiry) {
		if err := saveUserToken(tokenPath, updated); err != nil {
			return "", err
		}
	}
	return updated.AccessToken, nil
}

func saveUserToken(path string, token *oauth2.Token) error {
	data, err := json.MarshalIndent(token, "", "  ")
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(filepath.Dir(path), ".google-user-token-*")
	if err != nil {
		return fmt.Errorf("save Google user token: %w", err)
	}
	defer os.Remove(tmp.Name())
	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Chmod(0600); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmp.Name(), path)
}
