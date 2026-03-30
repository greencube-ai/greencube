import React from 'react';
import { AppProvider, useApp } from './context/AppContext';
import { OnboardingModal } from './components/OnboardingModal';
import { SettingsPanel } from './components/SettingsPanel';

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { hasError: boolean; error: string }
> {
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { hasError: false, error: '' };
  }
  static getDerivedStateFromError(error: Error) {
    return { hasError: true, error: error.message };
  }
  render() {
    if (this.state.hasError) {
      return (
        <div className="flex flex-col items-center justify-center min-h-screen gap-4">
          <div className="text-[var(--status-error)] text-lg">Something went wrong</div>
          <div className="text-[var(--text-muted)] text-sm">{this.state.error}</div>
          <button onClick={() => window.location.reload()}
            className="px-4 py-2 bg-[var(--accent)] text-black rounded-lg hover:bg-[var(--accent-hover)]">Reload</button>
        </div>
      );
    }
    return this.props.children;
  }
}

function AppContent() {
  const { state } = useApp();

  if (state.loading) {
    return (
      <div className="flex items-center justify-center min-h-screen">
        <div className="text-[var(--text-muted)]">Loading...</div>
      </div>
    );
  }

  if (state.config && !state.config.ui.onboarding_complete) {
    return <OnboardingModal />;
  }

  return <SettingsPanel />;
}

export default function App() {
  return (
    <ErrorBoundary>
      <AppProvider>
        <AppContent />
      </AppProvider>
    </ErrorBoundary>
  );
}
