import { ViewerAnalytics } from "../../../components/ViewerAnalytics";

/** Creator Studio → Analytics (docs/ANALYTICS.md): the streamer's own viewer analytics. */
export default function StudioAnalytics() {
  return <><h1>Analytics</h1><ViewerAnalytics endpoint="/api/me/analytics" /></>;
}
