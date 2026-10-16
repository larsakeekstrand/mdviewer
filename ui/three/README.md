# Vendored three.js

Version: three@0.170.0 (r170). This release ships a single self-contained `build/three.module.min.js` (no `three.core.min.js` split).

Sources (jsDelivr, `https://cdn.jsdelivr.net/npm/three@0.170.0/`):

- `build/three.module.min.js` -> `three.module.min.js`
- `examples/jsm/controls/OrbitControls.js`
- `examples/jsm/loaders/STLLoader.js`
- `examples/jsm/loaders/3MFLoader.js`
- `examples/jsm/loaders/OBJLoader.js`
- `examples/jsm/libs/fflate.module.js`

Addons are unminified as shipped. Only import specifiers were rewritten:

- `from 'three'` -> `from './three.module.min.js'` (OrbitControls, STLLoader, 3MFLoader, OBJLoader)
- `'../libs/fflate.module.js'` -> `'./fflate.module.js'` (3MFLoader)

Check: `grep -nE "from ['\"](three|\.\./)" ui/three/*.js` must print nothing.

Re-vendoring must repeat the rewrite; import maps are blocked by CSP `script-src 'self'`. If a newer release splits out `three.core.min.js`, vendor it too.
