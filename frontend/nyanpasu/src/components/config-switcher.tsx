import { useState, useCallback, useEffect, useMemo } from 'react'
import { 
  Button, 
  Select, 
  SelectChangeEvent, 
  MenuItem, 
  FormControl, 
  InputLabel, 
  Alert, 
  Snackbar, 
  CircularProgress, 
  Box, 
  Typography,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  DialogContentText,
  Chip
} from '@mui/material'
import { useConfigSync } from '../hooks/use-config-sync'
import { useProfile } from '@nyanpasu/interface/ipc/use-profile'

export interface ConfigSwitcherProps {
  className?: string
}

export const ConfigSwitcher = ({ className }: ConfigSwitcherProps) => {
  const {
    isSwitching,
    switchError,
    multiCoreWarning,
    currentConfigName,
    recoveryAttempts,
    isRecovering,
    lastSuccessTime,
    configSwitchHistory,
    switchConfig,
    reloadCore,
    recoverFromError,
    checkRunningCores,
    profileData
  } = useConfigSync()
  
  const { query: profileQuery } = useProfile()
  const [selectedConfig, setSelectedConfig] = useState('')
  const [snackbarOpen, setSnackbarOpen] = useState(false)
  const [snackbarMessage, setSnackbarMessage] = useState('')
  const [snackbarSeverity, setSnackbarSeverity] = useState<'success' | 'error' | 'warning' | 'info'>('info')
  const [confirmReloadOpen, setConfirmReloadOpen] = useState(false)
  const [confirmSwitchOpen, setConfirmSwitchOpen] = useState(false)
  const [pendingConfig, setPendingConfig] = useState('')

  // Get available profiles (优先使用 Hook 中的数据)
  const availableProfiles = useMemo(() => {
    return profileData?.items || profileQuery.data?.items || []
  }, [profileData?.items, profileQuery.data?.items])

  // Get current config info
  const currentConfigInfo = useMemo(() => {
    if (!currentConfigName || !availableProfiles.length) return null
    return availableProfiles.find((p: any) => p.uid === currentConfigName)
  }, [currentConfigName, availableProfiles])

  // Initialize selected config based on current config
  useEffect(() => {
    if (currentConfigName && !selectedConfig) {
      setSelectedConfig(currentConfigName)
    } else if (!currentConfigName && availableProfiles.length > 0 && !selectedConfig) {
      // 如果没有当前配置，选择第一个可用的
      setSelectedConfig(availableProfiles[0].uid || '')
    }
  }, [currentConfigName, selectedConfig, availableProfiles])

  // Handle config selection change
  const handleConfigChange = (event: SelectChangeEvent<string>) => {
    const newConfig = event.target.value
    
    // 如果选择的是当前配置，不需要切换
    if (newConfig === currentConfigName) {
      setSelectedConfig(newConfig || '')
      return
    }

    setSelectedConfig(newConfig || '')
    
    // If there are multi-core warnings, show confirmation dialog
    if (multiCoreWarning) {
      setPendingConfig(newConfig)
      setConfirmSwitchOpen(true)
    } else {
      handleSwitchConfig(newConfig)
    }
  }

  // Handle actual config switch
  const handleSwitchConfig = useCallback(async (configName: string) => {
    if (!configName || configName === currentConfigName) {
      return
    }

    try {
      const profile = availableProfiles.find((p: any) => p.uid === configName)
      await switchConfig(configName)
      showSnackbar(
        `Configuration switched to "${profile?.name || configName}" successfully!`, 
        'success'
      )
    } catch (error: any) {
      showSnackbar(`Failed to switch configuration: ${error.message}`, 'error')
      // 恢复到之前的选择
      if (currentConfigName) {
        setSelectedConfig(currentConfigName || '')
      }
    }
  }, [switchConfig, currentConfigName, availableProfiles])

  // Handle reload core action
  const handleReloadCore = useCallback(async () => {
    try {
      await reloadCore()
      showSnackbar('Core reloaded successfully!', 'success')
    } catch (error: any) {
      showSnackbar(`Failed to reload core: ${error.message}`, 'error')
    }
    setConfirmReloadOpen(false)
  }, [reloadCore])

  // Handle recovery action
  const handleRecover = useCallback(async () => {
    try {
      await recoverFromError()
      showSnackbar('Recovery attempt completed!', 'info')
    } catch (error: any) {
      showSnackbar(`Recovery failed: ${error.message}`, 'error')
    }
  }, [recoverFromError])

  // Show snackbar notification
  const showSnackbar = (message: string, severity: 'success' | 'error' | 'warning' | 'info') => {
    setSnackbarMessage(message)
    setSnackbarSeverity(severity)
    setSnackbarOpen(true)
  }

  // Close snackbar
  const handleSnackbarClose = () => {
    setSnackbarOpen(false)
  }

  // Handle confirm switch dialog
  const handleConfirmSwitch = () => {
    handleSwitchConfig(pendingConfig)
    setConfirmSwitchOpen(false)
    setPendingConfig('')
  }

  // Handle cancel switch dialog
  const handleCancelSwitch = () => {
    setConfirmSwitchOpen(false)
    setPendingConfig('')
    // Reset selection to current config
      if (currentConfigName) {
        setSelectedConfig(currentConfigName || '')
      }
  }

  // Format last success time with better type safety
  const formatLastSuccessTime = (timestamp: number | null) => {
    if (!timestamp) return 'Never'
    
    const date = new Date(timestamp)
    if (isNaN(date.getTime())) return 'Invalid time'
    
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    
    if (diff < 60000) return 'Just now'
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`
    if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`
    return date.toLocaleString()
  }

  return (
    <div className={className}>
      <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
        {/* Multi-core warning */}
        {multiCoreWarning && (
          <Alert severity="warning">
            <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
              <Box>
                <Typography variant="subtitle2" sx={{ fontWeight: 'bold', mb: 0.5 }}>
                  Multiple Clash Cores Detected
                </Typography>
                <Typography variant="body2">
                  This may cause configuration conflicts and unexpected behavior.
                </Typography>
              </Box>
              <Box sx={{ display: 'flex', gap: 1, ml: 2, flexShrink: 0 }}>
                <Button 
                  size="small" 
                  variant="outlined" 
                  onClick={() => setConfirmReloadOpen(true)}
                  disabled={isSwitching || isRecovering}
                >
                  Reload Core
                </Button>
                <Button 
                  size="small" 
                  variant="outlined" 
                  onClick={checkRunningCores}
                  disabled={isSwitching || isRecovering}
                >
                  Recheck
                </Button>
              </Box>
            </Box>
          </Alert>
        )}

        {/* Recovery status */}
        {(recoveryAttempts > 0 || isRecovering) && (
          <Alert severity={isRecovering ? "info" : recoveryAttempts >= 3 ? "error" : "warning"}>
            <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <Typography variant="body2">
                {isRecovering 
                  ? "Recovery in progress..." 
                  : `Recovery attempts: ${recoveryAttempts}/3`
                }
                {recoveryAttempts >= 3 && " - Manual intervention may be required"}
              </Typography>
              {!isRecovering && (
                <Button 
                  size="small" 
                  variant="outlined" 
                  onClick={handleRecover}
                  disabled={recoveryAttempts >= 3}
                >
                  Try Recovery
                </Button>
              )}
            </Box>
          </Alert>
        )}

        {/* Configuration switcher */}
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 2 }}>
          <FormControl fullWidth size="small">
            <InputLabel id="config-select-label">Configuration Profile</InputLabel>
            <Select
              labelId="config-select-label"
              id="config-select"
              value={selectedConfig}
              label="Configuration Profile"
              onChange={handleConfigChange}
              disabled={isSwitching || isRecovering || availableProfiles.length === 0}
            >
              {availableProfiles.length === 0 ? (
                <MenuItem disabled value="">
                  No profiles available
                </MenuItem>
              ) : (
                availableProfiles.map((profile: any) => (
                  <MenuItem key={profile.uid} value={profile.uid}>
                    <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                      {profile.name}
                      {profile.uid === currentConfigName && (
                        <Chip label="Active" size="small" color="primary" />
                      )}
                    </Box>
                  </MenuItem>
                ))
              )}
            </Select>
          </FormControl>

          {(isSwitching || isRecovering) && (
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
              <CircularProgress size={20} />
              <Typography variant="caption">
                {isRecovering ? 'Recovering...' : 'Switching...'}
              </Typography>
            </Box>
          )}
        </Box>

        {/* Current config info */}
        {currentConfigInfo && (
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 2 }}>
            <Typography variant="body2" color="text.primary">
              Current: <strong>{currentConfigInfo.name}</strong>
            </Typography>
            <Chip 
              label="Active" 
              size="small" 
              color="success" 
              variant="outlined"
            />
          </Box>
        )}

        {/* Error display */}
        {switchError && (
          <Alert severity="error">
            <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
              <Box>
                <Typography variant="subtitle2" sx={{ fontWeight: 'bold', mb: 0.5 }}>
                  Configuration Error
                </Typography>
                <Typography variant="body2">{switchError}</Typography>
              </Box>
              <Box sx={{ display: 'flex', gap: 1, ml: 2, flexShrink: 0 }}>
                <Button 
                  size="small" 
                  variant="outlined" 
                  onClick={handleRecover}
                  disabled={isRecovering}
                >
                  Try Recovery
                </Button>
                <Button 
                  size="small" 
                  variant="outlined" 
                  onClick={() => setConfirmReloadOpen(true)}
                  disabled={isSwitching || isRecovering}
                >
                  Reload Core
                </Button>
              </Box>
            </Box>
          </Alert>
        )}

        {/* Status info */}
        <Box sx={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <Typography variant="caption" color="text.secondary">
            Last sync: {formatLastSuccessTime(lastSuccessTime)}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Profiles: {availableProfiles.length}
          </Typography>
        </Box>
      </Box>

      {/* Snackbar for notifications */}
      <Snackbar
        open={snackbarOpen}
        autoHideDuration={6000}
        onClose={handleSnackbarClose}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }}
      >
        <Alert 
          onClose={handleSnackbarClose} 
          severity={snackbarSeverity}
          sx={{ width: '100%' }}
          variant="filled"
        >
          {snackbarMessage}
        </Alert>
      </Snackbar>

      {/* Confirm reload dialog */}
      <Dialog
        open={confirmReloadOpen}
        onClose={() => setConfirmReloadOpen(false)}
        maxWidth="sm"
        fullWidth
      >
        <DialogTitle>Confirm Core Reload</DialogTitle>
        <DialogContent>
          <DialogContentText>
            Reloading the Clash core will temporarily disconnect all proxy connections 
            and restart the network engine. This usually takes 10-15 seconds.
          </DialogContentText>
          <DialogContentText sx={{ mt: 2, fontWeight: 'bold' }}>
            Are you sure you want to proceed?
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setConfirmReloadOpen(false)}>Cancel</Button>
          <Button 
            onClick={handleReloadCore} 
            color="primary" 
            variant="contained"
            autoFocus
          >
            Reload Core
          </Button>
        </DialogActions>
      </Dialog>

      {/* Confirm switch with multi-core warning dialog */}
      <Dialog
        open={confirmSwitchOpen}
        onClose={handleCancelSwitch}
        maxWidth="sm"
        fullWidth
      >
        <DialogTitle>Multi-Core Conflict Warning</DialogTitle>
        <DialogContent>
          <DialogContentText>
            Multiple Clash cores are currently running, which may cause configuration 
            conflicts and unpredictable behavior.
          </DialogContentText>
          <DialogContentText sx={{ mt: 2 }}>
            Switching configurations in this state may result in:
          </DialogContentText>
          <Box component="ul" sx={{ mt: 1, pl: 2 }}>
            <li>Connection failures</li>
            <li>Proxy rule conflicts</li>
            <li>Inconsistent routing behavior</li>
          </Box>
          <DialogContentText sx={{ mt: 2, fontWeight: 'bold' }}>
            Do you want to proceed with the configuration switch?
          </DialogContentText>
        </DialogContent>
        <DialogActions>
          <Button onClick={handleCancelSwitch}>Cancel</Button>
          <Button 
            onClick={handleConfirmSwitch} 
            color="warning" 
            variant="contained"
            autoFocus
          >
            Proceed Anyway
          </Button>
        </DialogActions>
      </Dialog>
    </div>
  )
}