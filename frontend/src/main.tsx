import { createRoot } from 'react-dom/client'
import './api'
import App from './App'
import './style.css'

const container = document.getElementById('app')
if (!container) throw new Error('Application mount element is missing')

createRoot(container).render(<App />)
