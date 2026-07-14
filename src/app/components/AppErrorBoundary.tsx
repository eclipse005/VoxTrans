import { Component, type ErrorInfo, type ReactNode } from "react";
import { withTranslation, type WithTranslation } from "react-i18next";
import { reportError } from "../utils/errors";

type AppErrorBoundaryProps = {
  children: ReactNode;
} & WithTranslation;

type AppErrorBoundaryState = {
  hasError: boolean;
};

export class AppErrorBoundary extends Component<AppErrorBoundaryProps, AppErrorBoundaryState> {
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

    const { t } = this.props;

    return (
      <div className="app-error-boundary">
        <div className="app-error-boundary-card">
          <h1 className="apple-heading-small">{t("common:appError.title")}</h1>
          <p className="apple-body">
            {t("common:appError.description")}
          </p>
          <button className="apple-button" onClick={this.handleReload}>
            {t("common:appError.reload")}
          </button>
        </div>
      </div>
    );
  }
}

const AppErrorBoundaryWithTranslation = withTranslation("common")(AppErrorBoundary);
export default AppErrorBoundaryWithTranslation;
