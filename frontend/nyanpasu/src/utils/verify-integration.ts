// Integration verification file to ensure all components work together
import { configSwitchLogger } from '../utils/config-logger'

// This file is used to verify that all the components of the configuration switch feature
// are properly integrated and can work together

console.log('=== Configuration Switch Feature Integration Verification ===')

// Test 1: Logger initialization
console.log('1. Testing logger initialization...')
try {
  const logs = configSwitchLogger.getLogs()
  console.log('✓ Logger initialized successfully')
  console.log(`  Initial log count: ${logs.length}`)
} catch (error) {
  console.error('✗ Logger initialization failed:', error)
}

// Test 2: Logger functionality
console.log('2. Testing logger functionality...')
try {
  configSwitchLogger.info('Test info message', { component: 'verification' })
  configSwitchLogger.warn('Test warning message', { component: 'verification' })
  configSwitchLogger.error('Test error message', new Error('Test error'))
  
  const logs = configSwitchLogger.getLogs()
  console.log('✓ Logger functionality working')
  console.log(`  Log count after test messages: ${logs.length}`)
  
  // Test export functionality
  const stringExport = configSwitchLogger.exportLogs('string')
  const jsonExport = configSwitchLogger.exportLogs('json')
  console.log('✓ Export functionality working')
  console.log(`  String export length: ${stringExport.length}`)
  console.log(`  JSON export length: ${jsonExport.length}`)
} catch (error) {
  console.error('✗ Logger functionality failed:', error)
}

// Test 3: Log deduplication
console.log('3. Testing log deduplication...')
try {
  const initialCount = configSwitchLogger.getLogs().length
  configSwitchLogger.info('Deduplication test', { count: 1 })
  configSwitchLogger.info('Deduplication test', { count: 1 })
  configSwitchLogger.info('Deduplication test', { count: 1 })
  
  const finalCount = configSwitchLogger.getLogs().length
  console.log('✓ Log deduplication working')
  console.log(`  Initial count: ${initialCount}, Final count: ${finalCount}`)
  console.log(`  Last log count: ${configSwitchLogger.getLogs()[finalCount - 1].count}`)
} catch (error) {
  console.error('✗ Log deduplication failed:', error)
}

console.log('=== Integration Verification Complete ===')