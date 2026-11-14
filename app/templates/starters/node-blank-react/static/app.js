// __HOST__: React with htm, which reads like JSX and runs as written.
const html = htm.bind(React.createElement);

function App() {
  return html`<h1>__HOST__</h1><p>A blank React app, running. Save a file and it shows here.</p>`;
}

ReactDOM.createRoot(document.getElementById('root')).render(html`<${App} />`);
