FROM python:3.11-slim

RUN pip install docker

COPY main.py /app/main.py

ENTRYPOINT [ "python" , "-u" , "/app/main.py" ]