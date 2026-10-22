/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        chronos: {
          950: '#07090e',
          900: '#0c1017',
          850: '#111622',
          800: '#171f2f',
          700: '#232f46',
          600: '#344565',
          cyan: '#00d2ff',
          neon: '#00f2fe',
          danger: '#ff3366',
          warning: '#ffaa00',
          success: '#00e676',
        }
      },
      fontFamily: {
        mono: ['JetBrains Mono', 'Fira Code', 'Consolas', 'monospace'],
        sans: ['Inter', 'system-ui', 'sans-serif'],
      }
    },
  },
  plugins: [],
}
