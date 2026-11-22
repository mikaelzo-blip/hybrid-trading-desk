# syntax=docker/dockerfile:1
FROM python:3.12-slim
WORKDIR /app
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1
COPY pyproject.toml* ./
RUN pip install --no-cache-dir --upgrade pip && \
    if [ -f pyproject.toml ]; then pip install --no-cache-dir .; fi
COPY app ./app
ENV AI_WORKER_PORT=8000
EXPOSE 8000
CMD ["python", "-m", "uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
