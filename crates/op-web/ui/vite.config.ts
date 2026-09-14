import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";
import { componentTagger } from "lovable-tagger";

// Backend gRPC-Web and API target (adjust for your environment)
// REST on op-web :8080; gRPC-Web / reflection on op-grpc-bridge :8090.
const API_TARGET = process.env.VITE_API_TARGET || "http://127.0.0.1:8080";
const GRPC_TARGET = process.env.VITE_GRPC_TARGET || "http://127.0.0.1:8090";

// https://vitejs.dev/config/
export default defineConfig(({ mode }) => ({
  server: {
    host: "::",
    port: 8080,
    hmr: {
      overlay: false,
    },
    proxy: {
      // Proxy REST API requests → op-web
      "/api": {
        target: API_TARGET,
        changeOrigin: true,
        secure: false,
      },
      // Proxy gRPC-Web requests → op-grpc-bridge
      "/operation.v1": {
        target: GRPC_TARGET,
        changeOrigin: true,
        secure: false,
        ws: false,
      },
      "/operation.registry.v1": {
        target: GRPC_TARGET,
        changeOrigin: true,
        secure: false,
        ws: false,
      },
      "/grpc.reflection": {
        target: GRPC_TARGET,
        changeOrigin: true,
        secure: false,
        ws: false,
      },
    },
  },
  plugins: [react(), mode === "development" && componentTagger()].filter(Boolean),
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
}));
