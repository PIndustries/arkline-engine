import init from './arkline-client.js';init().catch(e=>{document.getElementById('loading').textContent='Arkline could not start. This browser needs WebAssembly and WebGL 2. '+e;});
