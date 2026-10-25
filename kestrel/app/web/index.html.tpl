<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Kestrel</title>
    <style>
      html, body { margin: 0; width: 100%; height: 100%; overflow: hidden; background: #07121b; color: #d8ecf0; font-family: sans-serif; }
      #game { width: 100vw; height: 100vh; }
      #status { position: absolute; top: 1rem; left: 1rem; }
      canvas { display: block; }
    </style>
  </head>
  <body>
    <div id="game"><canvas id="kestrel"></canvas></div>
    <div id="status">Loading Kestrel…</div>
    <script type="module">
      import init from './{{GAME_JS}}';
      window.kestrelWebAudioPlay = () => {};
      const AudioContextClass = window.AudioContext || window.webkitAudioContext;
      if (AudioContextClass) {
        const audio = new AudioContextClass();
        async function loadCue(name, kind) {
          try {
            const response = await fetch(`./${name}`);
            if (!response.ok) throw new Error(`cue ${kind}: HTTP ${response.status}`);
            return await audio.decodeAudioData(await response.arrayBuffer());
          } catch (error) {
            console.warn('Could not load a sound cue:', error);
            return null;
          }
        }
        async function loadCues() {
          try {
            const response = await fetch('./cues.json');
            if (!response.ok) throw new Error(`cues.json: HTTP ${response.status}`);
            const names = await response.json();
            return await Promise.all(names.map(loadCue));
          } catch (error) {
            console.warn('Could not load the sound bank:', error);
            return [];
          }
        }
        const cues = loadCues();

        const enableAudio = () => { void audio.resume().catch(console.warn); };
        window.addEventListener('pointerdown', enableAudio, { passive: true });
        window.addEventListener('keydown', enableAudio);
        window.kestrelWebAudioPlay = async kind => {
          const buffers = await cues;
          if (!Number.isInteger(kind) || kind < 0 || kind >= buffers.length) return;
          const buffer = buffers[kind];
          if (!buffer) return;
          const source = audio.createBufferSource();
          source.buffer = buffer;
          source.connect(audio.destination);
          source.start();
        };
      }
      init().then(() => document.getElementById('status').remove())
        .catch(error => {
          document.getElementById('status').textContent = `Kestrel could not start: ${error}`;
          console.error(error);
        });
    </script>
  </body>
</html>
