// Small Linux ABI boundary: ALSA owns its opaque types; Rust owns the UI.
#include <alsa/asoundlib.h>
#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <termios.h>
#include <unistd.h>

int mk_open(snd_seq_t **seq) {
    // Supply our own configuration so neither /usr/share/alsa nor ~/.asoundrc
    // is needed. The hw sequencer backend is linked into libasound.a.
    const char config[] = "seq.midi_keys { type hw }";
    snd_config_t *root = NULL;
    int result = snd_config_load_string(&root, config, sizeof(config) - 1);
    if (result < 0) return result;
    result = snd_seq_open_lconf(seq, "midi_keys", SND_SEQ_OPEN_DUPLEX,
                               SND_SEQ_NONBLOCK, root);
    snd_config_delete(root);
    if (result < 0) return result;
    result = snd_seq_set_client_name(*seq, "MIDI Keys");
    if (result >= 0) {
        result = snd_seq_create_simple_port(*seq, "Input",
            SND_SEQ_PORT_CAP_WRITE | SND_SEQ_PORT_CAP_SUBS_WRITE,
            SND_SEQ_PORT_TYPE_MIDI_GENERIC | SND_SEQ_PORT_TYPE_APPLICATION);
    }
    if (result < 0) {
        snd_seq_close(*seq);
        *seq = NULL;
    }
    return result;
}

void mk_close(snd_seq_t *seq) { snd_seq_close(seq); }
const char *mk_error(int code) { return snd_strerror(code); }

// Enumerate readable subscribable inputs; omit ALSA's system timer/announce.
int mk_port(snd_seq_t *seq, int index, int *client, int *port,
            char *name, size_t size) {
    snd_seq_client_info_t *ci;
    snd_seq_port_info_t *pi;
    snd_seq_client_info_alloca(&ci);
    snd_seq_port_info_alloca(&pi);
    snd_seq_client_info_set_client(ci, -1);
    while (snd_seq_query_next_client(seq, ci) >= 0) {
        int id = snd_seq_client_info_get_client(ci);
        if (id == 0 || id == snd_seq_client_id(seq)) continue;
        snd_seq_port_info_set_client(pi, id);
        snd_seq_port_info_set_port(pi, -1);
        while (snd_seq_query_next_port(seq, pi) >= 0) {
            unsigned caps = snd_seq_port_info_get_capability(pi);
            unsigned required = SND_SEQ_PORT_CAP_READ | SND_SEQ_PORT_CAP_SUBS_READ;
            if ((caps & required) != required) continue;
            if (index-- != 0) continue;
            *client = id;
            *port = snd_seq_port_info_get_port(pi);
            snprintf(name, size, "%s / %s", snd_seq_client_info_get_name(ci),
                     snd_seq_port_info_get_name(pi));
            return 1;
        }
    }
    return 0;
}

int mk_connect(snd_seq_t *seq, int client, int port) {
    return snd_seq_connect_from(seq, 0, client, port);
}

void mk_disconnect(snd_seq_t *seq, int client, int port) {
    snd_seq_disconnect_from(seq, 0, client, port);
}

// 0 = no event, 1 = MIDI, 2 = unrelated, 3 = disconnected, negative = error.
int mk_event(snd_seq_t *seq, unsigned char *message, int *client, int *port,
             int *event_type, const unsigned char **raw, size_t *raw_size) {
    snd_seq_event_t *event = NULL;
    int result = snd_seq_event_input(seq, &event);
    if (result == -EAGAIN) return 0;
    if (result < 0) return result;
    // Borrowed ALSA storage: Rust copies this before the next event read.
    *event_type = event->type;
    if ((event->flags & SND_SEQ_EVENT_LENGTH_MASK) != SND_SEQ_EVENT_LENGTH_FIXED) {
        *raw = event->data.ext.ptr;
        *raw_size = event->data.ext.len;
    } else {
        *raw = (const unsigned char *)&event->data;
        *raw_size = sizeof(event->data);
    }
    *client = event->source.client;
    *port = event->source.port;
    switch (event->type) {
    case SND_SEQ_EVENT_NOTEON:
    case SND_SEQ_EVENT_NOTEOFF:
        message[0] = (event->type == SND_SEQ_EVENT_NOTEON ? 0x90 : 0x80)
                     | event->data.note.channel;
        message[1] = event->data.note.note;
        message[2] = event->data.note.velocity;
        break;
    case SND_SEQ_EVENT_CONTROLLER:
        if (event->data.control.param > 127 || event->data.control.value < 0 ||
            event->data.control.value > 127) return 2;
        message[0] = 0xb0 | event->data.control.channel;
        message[1] = event->data.control.param;
        message[2] = event->data.control.value;
        break;
    case SND_SEQ_EVENT_RESET:
        message[0] = 0xff;
        message[1] = message[2] = 0;
        break;
    case SND_SEQ_EVENT_PORT_UNSUBSCRIBED:
        *client = event->data.connect.sender.client;
        *port = event->data.connect.sender.port;
        return 3;
    default:
        return 2;
    }
    return 1;
}

static struct termios saved_terminal;
static int terminal_active;
static volatile sig_atomic_t stopping;
static const int signals[] = {SIGINT, SIGTERM, SIGHUP};
static struct sigaction saved_actions[3];

static void stop(int signal_number) { (void)signal_number; stopping = 1; }

int mk_terminal_enter(void) {
    if (!isatty(STDIN_FILENO) || !isatty(STDOUT_FILENO)) return -ENOTTY;
    if (tcgetattr(STDIN_FILENO, &saved_terminal) < 0) return -errno;
    struct sigaction action = {0};
    action.sa_handler = stop;
    sigemptyset(&action.sa_mask);
    for (int i = 0; i < 3; ++i) {
        if (sigaction(signals[i], &action, &saved_actions[i]) < 0) {
            int error = errno;
            for (int j = 0; j < i; ++j) sigaction(signals[j], &saved_actions[j], NULL);
            return -error;
        }
    }
    struct termios raw = saved_terminal;
    raw.c_lflag &= ~(ECHO | ICANON | ISIG | IEXTEN);
    raw.c_iflag &= ~(IXON | ICRNL);
    raw.c_oflag &= ~OPOST;
    raw.c_cc[VMIN] = 0;
    raw.c_cc[VTIME] = 0;
    if (tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw) < 0) {
        int error = errno;
        for (int i = 0; i < 3; ++i) sigaction(signals[i], &saved_actions[i], NULL);
        return -error;
    }
    terminal_active = 1;
    return 0;
}

void mk_terminal_leave(void) {
    if (!terminal_active) return;
    tcsetattr(STDIN_FILENO, TCSAFLUSH, &saved_terminal);
    for (int i = 0; i < 3; ++i) sigaction(signals[i], &saved_actions[i], NULL);
    terminal_active = 0;
}

// Wait at most 2ms; -1 requests exit, 0 means no key, otherwise an input byte.
int mk_key(void) {
    if (stopping) return -1;
    struct pollfd input = {STDIN_FILENO, POLLIN, 0};
    int result = poll(&input, 1, 2);
    if (stopping) return -1;
    if (result < 0) return errno == EINTR ? 0 : -1;
    if (input.revents & (POLLHUP | POLLERR | POLLNVAL)) return -1;
    unsigned char key;
    if ((input.revents & POLLIN) && read(STDIN_FILENO, &key, 1) == 1) return key;
    return 0;
}

void mk_terminal_size(int *columns, int *rows) {
    struct winsize size;
    if (ioctl(STDOUT_FILENO, TIOCGWINSZ, &size) == 0 && size.ws_col && size.ws_row) {
        *columns = size.ws_col;
        *rows = size.ws_row;
    } else {
        *columns = 80;
        *rows = 24;
    }
}
