/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Where the console's Playground button points. */
  readonly VITE_PLAYGROUND_URL?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
