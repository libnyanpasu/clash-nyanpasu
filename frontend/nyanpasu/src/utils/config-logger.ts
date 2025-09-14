import { configSwitchHistoryAtom } from '@/store'
import { useAtomValue } from 'jotai'
import { useEffect, useState } from 'react'

// Log levels
export type LogLevel = 'debug' | 'info' | 'warn' | 'error'

// Log entry structure
export interface LogEntry {
  timestamp: number
  level: LogLevel
  message: string
  details?: Record<string, unknown> | string | Error
  count?: number
}

/**
 * Serialize details for logging
 */
const serializeDetails = (details?: LogEntry['details']) => {
  if (!details) return ''
  if (details instanceof Error) {
    return JSON.stringify({ 
      name: details.name, 
      message: details.message, 
      stack: details.stack 
    }, null, 2)
  }
  return typeof details === 'string' ? details : JSON.stringify(details, null, 2)
}

/**
 * Configuration switch logger utility
 * Provides logging capabilities for debugging and troubleshooting
 */
class ConfigSwitchLogger {
  private logs: LogEntry[] = []
  private maxLogs = 100
  private listeners: Array<(logs: LogEntry[]) => void> = []
  private persistTimeout: ReturnType<typeof setTimeout> | null = null

  constructor() {
    // Load persisted logs on initialization
    if (typeof window !== 'undefined') {
      this.loadLogs()
    }
  }

  /**
   * Log a message with the specified level
   */
  log(level: LogLevel, message: string, details?: Record<string, unknown> | string | Error) {
    // Check if this is a duplicate of the last log entry
    const lastEntry = this.logs[this.logs.length - 1]
    if (lastEntry && 
        lastEntry.message === message && 
        lastEntry.level === level &&
        JSON.stringify(lastEntry.details) === JSON.stringify(details)) {
      // Increment count instead of adding new entry
      lastEntry.count = (lastEntry.count || 1) + 1
      lastEntry.timestamp = Date.now()
    } else {
      // Add new entry
      const entry: LogEntry = {
        timestamp: Date.now(),
        level,
        message,
        details,
        count: 1
      }

      this.logs.push(entry)
      
      // Keep only the most recent logs
      if (this.logs.length > this.maxLogs) {
        this.logs = this.logs.slice(-this.maxLogs)
      }
    }

    // Notify listeners
    this.notifyListeners()

    // Also log to console for development with better compatibility
    if (typeof process !== 'undefined' && process.env && process.env.NODE_ENV === 'development') {
      const logFn = (console as any)[level] ?? console.log
      logFn(`[ConfigSwitch] ${message}`, details)
    }

    // Schedule persistence with debounce
    this.schedulePersistence()
  }

  /**
   * Log a debug message
   */
  debug(message: string, details?: Record<string, unknown> | string | Error) {
    this.log('debug', message, details)
  }

  /**
   * Log an info message
   */
  info(message: string, details?: Record<string, unknown> | string | Error) {
    this.log('info', message, details)
  }

  /**
   * Log a warning message
   */
  warn(message: string, details?: Record<string, unknown> | string | Error) {
    this.log('warn', message, details)
  }

  /**
   * Log an error message
   */
  error(message: string, details?: Record<string, unknown> | string | Error) {
    this.log('error', message, details)
  }

  /**
   * Get all logs
   */
  getLogs(): LogEntry[] {
    return [...this.logs]
  }

  /**
   * Get logs filtered by level
   */
  getLogsByLevel(level: LogLevel): LogEntry[] {
    return this.logs.filter(log => log.level === level)
  }

  /**
   * Get logs from the last N minutes
   */
  getRecentLogs(minutes: number = 5): LogEntry[] {
    const cutoff = Date.now() - (minutes * 60 * 1000)
    return this.logs.filter(log => log.timestamp >= cutoff)
  }

  /**
   * Clear all logs
   */
  clearLogs() {
    this.logs = []
    this.notifyListeners()
    this.schedulePersistence()
  }

  /**
   * Add a listener for log updates
   */
  addListener(listener: (logs: LogEntry[]) => void) {
    this.listeners.push(listener)
  }

  /**
   * Remove a listener
   */
  removeListener(listener: (logs: LogEntry[]) => void) {
    this.listeners = this.listeners.filter(l => l !== listener)
  }

  /**
   * Notify all listeners of log updates
   */
  private notifyListeners() {
    this.listeners.forEach(listener => listener([...this.logs]))
  }

  /**
   * Export logs in the specified format
   */
  exportLogs(format: 'string' | 'json' = 'string'): string {
    if (format === 'json') {
      return JSON.stringify(this.logs, null, 2)
    }
    
    // Default string format with local time
    return this.logs
      .map(log => {
        const date = new Date(log.timestamp).toLocaleString()
        const countStr = log.count && log.count > 1 ? ` (x${log.count})` : ''
        const details = serializeDetails(log.details)
        return `[${date}] ${log.level.toUpperCase()}: ${log.message}${countStr}${details ? `
${details}` : ''}`
      })
      .join(`

`)
  }

  /**
   * Schedule persistence with debounce
   */
  private schedulePersistence() {
    // Only run in browser environment
    if (typeof window === 'undefined') return
    
    if (this.persistTimeout) {
      clearTimeout(this.persistTimeout)
    }
    
    this.persistTimeout = setTimeout(() => {
      this.persistLogs()
      this.persistTimeout = null
    }, 1000) // Debounce for 1 second
  }

  /**
   * Persist logs to localStorage
   */
  private persistLogs() {
    // Only run in browser environment
    if (typeof window === 'undefined') return
    
    try {
      const logsToPersist = this.logs.slice(-50) // Only persist last 50 logs
      localStorage.setItem('config_switch_logs', JSON.stringify(logsToPersist))
    } catch (error) {
      console.warn('Failed to persist logs to localStorage:', error)
    }
  }

  /**
   * Load logs from localStorage
   */
  private loadLogs() {
    // Only run in browser environment
    if (typeof window === 'undefined') return
    
    try {
      const persistedLogs = localStorage.getItem('config_switch_logs')
      if (persistedLogs) {
        this.logs = JSON.parse(persistedLogs)
        this.notifyListeners()
      }
    } catch (error) {
      console.warn('Failed to load logs from localStorage:', error)
    }
  }
}

// Create a singleton instance
export const configSwitchLogger = new ConfigSwitchLogger()

/**
 * Hook to access configuration switch logs reactively
 */
export const useConfigSwitchLogs = () => {
  const [logs, setLogs] = useState<LogEntry[]>(configSwitchLogger.getLogs())

  useEffect(() => {
    const listener = (newLogs: LogEntry[]) => setLogs(newLogs)
    configSwitchLogger.addListener(listener)
    
    return () => {
      configSwitchLogger.removeListener(listener)
    }
  }, [])

  return logs
}

/**
 * Hook to access configuration switch logs filtered by level
 */
export const useConfigSwitchLogsByLevel = (level: LogLevel) => {
  const logs = useConfigSwitchLogs()
  return logs.filter(log => log.level === level)
}

/**
 * Hook to access configuration switch history from the store
 */
export const useConfigSwitchHistory = () => {
  return useAtomValue(configSwitchHistoryAtom)
}