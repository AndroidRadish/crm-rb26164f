import { useAuth } from '../auth'
import { Card, EmptyState } from './ui'

// 功能门：无授权隐藏入口（体验层；后端 403 是底线，两层都要）
export default function FeatureGate({ feature, children }) {
  const { hasFeature } = useAuth()
  return hasFeature(feature) ? children : null
}

// 直链访问被关功能时的提示页（spec：一句话提示即可，不做营销化）
export function FeatureNotice() {
  return (
    <Card className="u-max-w-sm u-mt-5 u-mx-auto">
      <EmptyState
        inline
        title="当前版本不含此功能"
        description="当前版本不含此功能，联系客服升级。"
      />
    </Card>
  )
}
