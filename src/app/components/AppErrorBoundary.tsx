import { Component, type ErrorInfo, type ReactNode } from "react";
import { reportError } from "../utils/errors";

type AppErrorBoundaryProps = {
  children: ReactNode;
};

type AppErrorBoundaryState = {
  hasError: boolean;
};

export default class AppErrorBoundary extends Component<AppErrorBoundaryProps, AppErrorBoundaryState> {
  state: AppErrorBoundaryState = {
    hasError: false,
  };

  static getDerivedStateFromError(): AppErrorBoundaryState {
    return { hasError: true };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    reportError(error, "react.render");
    reportError(errorInfo.componentStack, "react.componentStack");
  }

  private handleReload = () => {
    window.location.reload();
  };

  render() {
    if (!this.state.hasError) {
      return this.props.children;
    }

    return (
      <div className="app-error-boundary">
        <div className="app-error-boundary-card">
          <h1 className="apple-heading-small">应用发生异常</h1>
          <p className="apple-body">
            当前页面渲染失败。你可以点击重试刷新应用，若问题持续出现，请把控制台日志发给我排查。
          </p>
          <button className="apple-button" onClick={this.handleReload}>
            重新加载
          </button>
        </div>
      </div>
    );
  }
}
