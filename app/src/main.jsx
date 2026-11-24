import React from 'react'
import { createRoot } from 'react-dom/client'
import App from './App.jsx'
import { L } from './i18n.js'
import './styles.css'

// a display error shows a message with a way back instead of an empty window
class Guard extends React.Component {
  constructor(props) {
    super(props)
    this.state = { err: null }
  }
  static getDerivedStateFromError(err) {
    return { err }
  }
  componentDidCatch(err, info) {
    console.error(err, info?.componentStack)
  }
  render() {
    if (!this.state.err) return this.props.children
    return (
      <div className="crash">
        <h2>{L('Terjadi kesalahan tampilan', 'Something went wrong on screen')}</h2>
        <p>{L('Pekerjaan yang belum disimpan mungkin hilang. Klik muat ulang untuk melanjutkan.', 'Unsaved work may be lost. Click reload to continue.')}</p>
        <pre>{String(this.state.err?.stack || this.state.err)}</pre>
        <button className="btn primary" type="button" onClick={() => window.location.reload()}>{L('Muat ulang', 'Reload')}</button>
      </div>
    )
  }
}

createRoot(document.getElementById('root')).render(<Guard><App /></Guard>)
