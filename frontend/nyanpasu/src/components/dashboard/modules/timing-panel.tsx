import { useColorForDelay } from "@/hooks/theme";
import { Paper } from "@mui/material";
import Grid from "@mui/material/Unstable_Grid2";

function LatencyTag({ name, value }: { name: string; value: number }) {
  const color = useColorForDelay(value);

  return (
    <div className="flex justify-between gap-1">
      <div className="min-w-20 font-bold">{name}:</div>

      <div className="min-w-16 text-end" style={{ color }}>
        {value ? `${value.toFixed(0)} ms` : "Timeout"}
      </div>
    </div>
  );
}

export const TimingPanel = ({ data }: { data: { [key: string]: number } }) => {
  return (
    <Grid sm={12} md={4} lg={3} xl={3}>
      <Paper className="!h-full !rounded-3xl p-4">
        <div className="flex h-full flex-col justify-between">
          {Object.entries(data).map(([name, value]) => (
            <LatencyTag key={name} name={name} value={value} />
          ))}
        </div>
      </Paper>
    </Grid>
  );
};

export default TimingPanel;
