import { useState } from 'react'
import { THEME_PRESET_SEEDS } from '@nyanpasu/theme/spec'
import {
  Checkbox,
  DateTimeField,
  FilterChip,
  HctColorPicker,
  HctColorPickerHexField,
  HctColorPickerPreset,
  HctColorPickerPresets,
  HctColorPickerSlider,
  HctColorPickerSliders,
  Input,
  NumberStepper,
  NumericInput,
  SearchField,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Slider,
  Switch,
  SwitchItem,
  Textarea,
} from '@nyanpasu/ui'
import { PageHeader, SpecimenCard } from '../../studio/stage'

export function InputsPage() {
  const [textValue, setTextValue] = useState('127.0.0.1:7890')
  const [portValue, setPortValue] = useState<number | null>(7890)
  const [searchValue, setSearchValue] = useState('')
  const [stepperValue, setStepperValue] = useState(4)
  const [dateTime, setDateTime] = useState('2026-11-27T09:30')
  const [selectValue, setSelectValue] = useState('mihomo')
  const [switchChecked, setSwitchChecked] = useState(true)
  const [switchItemChecked, setSwitchItemChecked] = useState(false)
  const [checkboxChecked, setCheckboxChecked] = useState(true)
  const [sliderValue, setSliderValue] = useState(64)
  const [chipTcp, setChipTcp] = useState(true)
  const [chipUdp, setChipUdp] = useState(false)
  const [pickerColor, setPickerColor] = useState('#3e64a7')

  return (
    <div className="flex flex-col gap-6" data-slot="gallery-inputs-page">
      <PageHeader
        eyebrow="Components · Inputs"
        title="Inputs, Pickers & Selection Controls"
        description="Filled and outlined fields, numeric steppers, native-accessible selects, switches, checkboxes, filter chips, sliders, and the Material HCT color picker."
        badges={[
          'Input',
          'SearchField',
          'NumberStepper',
          'DateTimeField',
          'Select',
          'Switch',
          'Checkbox',
          'Slider',
          'FilterChip',
          'HctColorPicker',
        ]}
      />

      <SpecimenCard
        title="Text, Numeric & Search Fields"
        description="Filled fields use surface-container-highest; outlined fields use semantic outline borders; underlined fields match lightweight Android 17 edit dialogs."
        code={`<Input variant="filled" label="Mixed Port" value={value} onChange={...} />
<Input variant="outlined" label="Controller Address" value={value} onChange={...} />
<Input variant="underlined" label="Device Name" value={value} onChange={...} />
<SearchField variant="filled" value={query} onValueChange={setQuery} placeholder="Search rules..." clearLabel="Clear" />`}
      >
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <Input
            variant="filled"
            label="Controller Address (Filled)"
            value={textValue}
            onChange={(event) => setTextValue(event.target.value)}
          />
          <Input
            variant="outlined"
            label="Controller Address (Outlined)"
            value={textValue}
            onChange={(event) => setTextValue(event.target.value)}
          />
          <Input
            variant="underlined"
            label="Device Name (Underlined)"
            value={textValue}
            onChange={(event) => setTextValue(event.target.value)}
          />
          <NumericInput
            variant="outlined"
            label="Mixed Port (NumericInput)"
            value={portValue}
            min={1024}
            max={65535}
            onChange={setPortValue}
          />
          <div className="md:col-span-2">
            <Textarea
              variant="underlined"
              label="System Proxy Bypass (Textarea)"
              rows={3}
              defaultValue="localhost;127.;192.168.;10.;172.16.;172.17.;172.18.;172.19.;172.20.;172.21.;172.22.;172.23.;172.24.;172.25.;172.26.;172.27.;172.28.;172.29.;172.30.;172.31.*"
            />
          </div>
          <div className="flex flex-col justify-center gap-2 md:col-span-2">
            <SearchField
              variant="basic"
              value={searchValue}
              onValueChange={setSearchValue}
              placeholder="Search connections or rules..."
              clearLabel="Clear search"
            />
          </div>
        </div>
      </SpecimenCard>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <SpecimenCard
          title="Select & DateTimeField"
          description="Compound Radix Select with floating label semantics alongside React Aria DateTimeField."
          code={`<Select variant="outlined" value={core} onValueChange={setCore}>
  <SelectTrigger>
    <SelectValue placeholder="Choose core" />
  </SelectTrigger>
  <SelectContent>
    <SelectItem value="mihomo">Mihomo</SelectItem>
    <SelectItem value="clash-rs">Clash RS</SelectItem>
  </SelectContent>
</Select>`}
        >
          <div className="flex flex-col gap-4">
            <Select
              variant="outlined"
              value={selectValue}
              onValueChange={setSelectValue}
            >
              <SelectTrigger>
                <SelectValue placeholder="Select proxy core" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="mihomo">Mihomo (Meta)</SelectItem>
                <SelectItem value="mihomo-alpha">Mihomo Alpha</SelectItem>
                <SelectItem value="clash-rs">Clash RS</SelectItem>
              </SelectContent>
            </Select>

            <DateTimeField
              label="Scheduled Subscription Refresh"
              value={dateTime}
              onChange={setDateTime}
              locale="en-US"
              openCalendarLabel="Open calendar"
              clearLabel="Clear date"
            />
          </div>
        </SpecimenCard>

        <SpecimenCard
          title="NumberStepper Variants"
          description="Outlined, filled, and tonal steppers supporting stacked and inline layouts."
          code={`<NumberStepper
  label="Retry Attempts"
  variant="tonal"
  layout="inline"
  value={count}
  min={0}
  max={10}
  decrementLabel="Decrease"
  incrementLabel="Increase"
  onChange={setCount}
/>`}
        >
          <div className="flex flex-col gap-4">
            <NumberStepper
              label="Outlined (inline)"
              variant="outlined"
              layout="inline"
              value={stepperValue}
              min={0}
              max={12}
              decrementLabel="Decrease"
              incrementLabel="Increase"
              onChange={setStepperValue}
            />
            <NumberStepper
              label="Filled (inline)"
              variant="filled"
              layout="inline"
              value={stepperValue}
              min={0}
              max={12}
              decrementLabel="Decrease"
              incrementLabel="Increase"
              onChange={setStepperValue}
            />
            <NumberStepper
              label="Tonal (inline)"
              variant="tonal"
              layout="inline"
              value={stepperValue}
              min={0}
              max={12}
              decrementLabel="Decrease"
              incrementLabel="Increase"
              onChange={setStepperValue}
            />
          </div>
        </SpecimenCard>
      </div>

      <SpecimenCard
        title="Switch, SwitchItem, Checkbox, FilterChip & Slider"
        description="Android 17 / M3E 52x32px Switch (24px thumb with symmetric 4px concentric inset, check/close icons, and motion spring) alongside Checkbox, FilterChip, and Slider."
        slots={[
          'switch',
          'switch-thumb',
          'switch-thumb-check',
          'switch-thumb-close',
          'slider',
        ]}
      >
        <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
          <div className="flex flex-col gap-4">
            <div className="flex flex-wrap items-center gap-4">
              <label className="flex items-center gap-2 text-sm">
                <Switch
                  checked={switchChecked}
                  onCheckedChange={setSwitchChecked}
                />
                <span>Allow LAN</span>
              </label>

              <label className="flex items-center gap-2 text-sm">
                <Switch checked loading aria-label="Loading switch" />
                <span>Syncing</span>
              </label>

              <label className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={checkboxChecked}
                  onCheckedChange={(checked) =>
                    setCheckboxChecked(checked === true)
                  }
                />
                <span>IPv6</span>
              </label>
            </div>

            <SwitchItem
              checked={switchItemChecked}
              onCheckedChange={setSwitchItemChecked}
            >
              Auto-launch on login (SwitchItem)
            </SwitchItem>

            <div className="flex flex-wrap items-center gap-2">
              <FilterChip pressed={chipTcp} onPressedChange={setChipTcp}>
                TCP
              </FilterChip>
              <FilterChip pressed={chipUdp} onPressedChange={setChipUdp}>
                UDP
              </FilterChip>
              <FilterChip defaultPressed>TLS 1.3</FilterChip>
              <FilterChip disabled>Disabled</FilterChip>
            </div>
          </div>

          <div className="flex flex-col justify-center gap-3">
            <div className="flex items-center justify-between text-sm">
              <span className="text-on-surface font-medium">
                Latency Threshold
              </span>
              <span className="text-on-surface-variant font-mono tabular-nums">
                {sliderValue} ms
              </span>
            </div>
            <Slider
              aria-label="Latency threshold"
              min={0}
              max={200}
              step={2}
              value={sliderValue}
              onValueChange={setSliderValue}
            />
            <Slider
              aria-label="Compact latency threshold"
              size="sm"
              min={0}
              max={200}
              step={2}
              value={sliderValue}
              onValueChange={setSliderValue}
            />
          </div>
        </div>
      </SpecimenCard>

      <SpecimenCard
        title="HctColorPicker (Standalone Sandbox)"
        description="Perceptually uniform Hue, Chroma, and Tone picker built on Material Color Utilities."
        code={`<HctColorPicker value={color} onValueChange={setColor}>
  <HctColorPickerPresets>...</HctColorPickerPresets>
  <HctColorPickerSliders>
    <HctColorPickerSlider channel="hue" label="Hue" />
    <HctColorPickerSlider channel="chroma" label="Chroma" />
    <HctColorPickerSlider channel="tone" label="Tone" />
  </HctColorPickerSliders>
  <HctColorPickerHexField label="Hex" />
</HctColorPicker>`}
      >
        <div className="max-w-md">
          <HctColorPicker value={pickerColor} onValueChange={setPickerColor}>
            <HctColorPickerPresets className="flex flex-wrap gap-2">
              {THEME_PRESET_SEEDS.map((preset) => (
                <HctColorPickerPreset
                  key={preset.hex}
                  value={preset.hex}
                  label={preset.name}
                />
              ))}
            </HctColorPickerPresets>
            <HctColorPickerSliders className="mt-4 flex flex-col gap-2">
              <HctColorPickerSlider channel="hue" label="Hue" />
              <HctColorPickerSlider channel="chroma" label="Chroma" />
              <HctColorPickerSlider channel="tone" label="Tone" />
            </HctColorPickerSliders>
            <div className="mt-4">
              <HctColorPickerHexField label="Selected hex" />
            </div>
          </HctColorPicker>
        </div>
      </SpecimenCard>
    </div>
  )
}
