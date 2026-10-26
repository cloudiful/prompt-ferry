import { useCallback, useEffect, useRef } from 'react'
import { BarChart, LineChart } from 'echarts/charts'
import {
  GridComponent,
  LegendComponent,
  TooltipComponent,
} from 'echarts/components'
import * as echarts from 'echarts/core'
import type { EChartsOption } from 'echarts'
import { CanvasRenderer } from 'echarts/renderers'

echarts.use([
  BarChart,
  CanvasRenderer,
  GridComponent,
  LegendComponent,
  LineChart,
  TooltipComponent,
])

export type UsageChartProps = {
  option: EChartsOption
}

export function UsageChart({ option }: UsageChartProps) {
  const chartElement = useRef<HTMLDivElement | null>(null)
  const chart = useRef<echarts.ECharts | null>(null)
  const optionRef = useRef(option)
  optionRef.current = option

  const renderChart = useCallback(() => {
    const element = chartElement.current
    if (!element || element.clientWidth <= 0 || element.clientHeight <= 0)
      return
    chart.current ??= echarts.init(element)
    chart.current.setOption(optionRef.current, true)
  }, [])

  useEffect(() => {
    renderChart()
  }, [option, renderChart])

  useEffect(() => {
    const element = chartElement.current
    if (!element) return
    const observer = new ResizeObserver(() => {
      if (element.clientWidth <= 0 || element.clientHeight <= 0) return
      if (chart.current) {
        chart.current.resize()
        chart.current.setOption(optionRef.current, true)
      } else {
        renderChart()
      }
    })
    observer.observe(element)
    renderChart()
    return () => {
      observer.disconnect()
      chart.current?.dispose()
      chart.current = null
    }
  }, [renderChart])

  return (
    <div
      aria-hidden="true"
      className="h-[20rem] min-h-[16rem] w-full min-w-0"
      data-usage-chart
      ref={chartElement}
    />
  )
}
